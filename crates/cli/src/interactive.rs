//! Interactive mode — scrollback-based TUI matching pi's visual style.
//!
//! Architecture (matching pi):
//! - Chat messages are printed to the scrollback buffer (scroll naturally)
//! - The editor at the bottom is the only part that uses raw mode
//! - Streaming assistant output appears in real-time
//! - Status bar shows model + context usage
//! - Tool calls show with * indicator and result previews

use anyhow::Result;
use std::io::Write;

use bb_core::agent::{self, DEFAULT_SYSTEM_PROMPT};
use bb_core::config;
use bb_core::types::*;
use bb_provider::anthropic::AnthropicProvider;
use bb_provider::openai::OpenAiProvider;
use bb_provider::registry::{ApiType, ModelRegistry};
use bb_provider::{CompletionRequest, Provider, RequestOptions, StreamEvent};
use bb_session::{context, store};
use bb_tools::{builtin_tools, Tool, ToolContext};
use bb_tui::editor::Editor;
use bb_tui::markdown::MarkdownRenderer;
use bb_tui::status;
use chrono::Utc;
use crossterm::style::{Attribute, Color, Stylize};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::login;
use crate::slash::{self, SlashResult};
use crate::Cli;

// ── Styling helpers (match pi's visual style) ───────────────────────

fn style_header(text: &str) -> String {
    format!("{}", text.with(Color::DarkGrey))
}

fn style_role_user() -> String {
    format!("{}", "You".with(Color::Blue).bold())
}

fn style_role_assistant(model: &str) -> String {
    format!(
        "{} {}",
        "Assistant".with(Color::Green).bold(),
        format!("({})", model).with(Color::DarkGrey),
    )
}

fn style_tool_call(name: &str) -> String {
    format!(
        "  {} {}",
        "*".with(Color::Yellow),
        name.bold(),
    )
}

fn style_tool_running(name: &str) -> String {
    format!(
        "  {} {} ",
        ".",
        name.with(Color::Cyan),
    )
}

fn style_tool_ok() -> String {
    format!("{}", "✓".with(Color::Green))
}

fn style_tool_err() -> String {
    format!("{}", "✗".with(Color::Red))
}

fn style_separator(width: usize) -> String {
    format!("{}", "─".repeat(width).with(Color::DarkGrey))
}

fn style_tool_args_preview(args: &serde_json::Value) -> String {
    let s = match args {
        serde_json::Value::Object(map) => {
            map.iter()
                .map(|(k, v)| {
                    let val = match v {
                        serde_json::Value::String(s) => {
                            if s.len() > 40 {
                                format!("\"{}...\"", &s[..40])
                            } else {
                                format!("\"{s}\"")
                            }
                        }
                        other => {
                            let s = other.to_string();
                            if s.len() > 40 { format!("{}...", &s[..40]) } else { s }
                        }
                    };
                    format!("{k}={val}")
                })
                .collect::<Vec<_>>()
                .join(", ")
        }
        _ => args.to_string(),
    };
    format!("{}", s.with(Color::DarkGrey))
}

fn style_dim(text: &str) -> String {
    format!("{}", text.with(Color::DarkGrey))
}

fn style_error(text: &str) -> String {
    format!("{}", text.with(Color::Red))
}

// ── Banner (matches pi's startup header) ────────────────────────────

fn print_banner(model_name: &str, context_window: u64) {
    let w = crossterm::terminal::size().map(|(w, _)| w as usize).unwrap_or(80);
    println!();
    println!(
        " {} v{}",
        "bb-agent".with(Color::Cyan).bold(),
        env!("CARGO_PKG_VERSION"),
    );
    println!(
        " {} {}",
        "escape".with(Color::DarkGrey),
        "to interrupt".with(Color::Grey),
    );
    println!(
        " {} {}",
        "ctrl+c".with(Color::DarkGrey),
        "to clear".with(Color::Grey),
    );
    println!(
        " {} {}",
        "ctrl+d".with(Color::DarkGrey),
        "to exit (empty)".with(Color::Grey),
    );
    println!(
        " {} {}",
        "/".with(Color::DarkGrey),
        "for commands".with(Color::Grey),
    );
    println!(
        " {} {}",
        "!".with(Color::DarkGrey),
        "to run bash".with(Color::Grey),
    );
    println!();
    println!(" {}", style_separator(w.saturating_sub(2)));
    println!();
}

// ── Main interactive mode ───────────────────────────────────────────

pub async fn run_interactive(cli: Cli) -> Result<()> {
    let cwd = std::fs::canonicalize(cli.cwd.as_deref().unwrap_or("."))?;
    let global_dir = config::global_dir();
    std::fs::create_dir_all(&global_dir)?;
    let artifacts_dir = global_dir.join("artifacts");
    std::fs::create_dir_all(&artifacts_dir)?;

    let db_path = global_dir.join("sessions.db");
    let conn = store::open_db(&db_path)?;
    let cwd_str = cwd.to_str().unwrap_or(".");

    let session_id = if cli.r#continue {
        let sessions = store::list_sessions(&conn, cwd_str)?;
        match sessions.first() {
            Some(s) => s.session_id.clone(),
            None => store::create_session(&conn, cwd_str)?,
        }
    } else {
        store::create_session(&conn, cwd_str)?
    };

    let (provider_name, model_id, _) = crate::run::parse_model_arg(
        cli.provider.as_deref(),
        cli.model.as_deref(),
    );

    let agents_md = crate::run::load_agents_md(&cwd);
    let system_prompt = agent::build_system_prompt(
        cli.system_prompt.as_deref().unwrap_or(DEFAULT_SYSTEM_PROMPT),
        agents_md.as_deref(),
    );

    let registry = ModelRegistry::new();
    let model = registry
        .find(&provider_name, &model_id)
        .cloned()
        .unwrap_or_else(|| bb_provider::registry::Model {
            id: model_id.clone(),
            name: model_id.clone(),
            provider: provider_name.clone(),
            api: ApiType::OpenaiCompletions,
            context_window: 128_000,
            max_tokens: 16384,
            reasoning: false,
            base_url: None,
            cost: Default::default(),
        });

    let api_key = cli
        .api_key
        .clone()
        .unwrap_or_else(|| login::resolve_api_key(&provider_name).unwrap_or_default());

    if api_key.is_empty() {
        eprintln!(
            " {}",
            format!(
                "[!] No API key for '{}'. Run `bb login {}` or set env var.",
                provider_name, provider_name,
            )
            .with(Color::Yellow)
        );
    }

    let base_url = model
        .base_url
        .clone()
        .unwrap_or_else(|| "https://api.openai.com/v1".into());

    let tools = builtin_tools();
    let tool_ctx = ToolContext {
        cwd: cwd.clone(),
        artifacts_dir,
        on_output: None,
    };
    let tool_defs: Vec<serde_json::Value> = tools
        .iter()
        .map(|t| {
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": t.name(),
                    "description": t.description(),
                    "parameters": t.parameters_schema(),
                }
            })
        })
        .collect();

    let provider: Box<dyn Provider> = match model.api {
        ApiType::AnthropicMessages => Box::new(AnthropicProvider::new()),
        _ => Box::new(OpenAiProvider::new()),
    };

    let mut editor = Editor::new("> ");

    // ── Startup ──
    print_banner(&model.name, model.context_window);

    // If --continue, display restored messages
    if cli.r#continue {
        if let Ok(ctx) = context::build_context(&conn, &session_id) {
            for msg in &ctx.messages {
                display_message(msg, &model.id);
            }
        }
    }

    // If initial prompt, run it
    if !cli.messages.is_empty() {
        let prompt = cli.messages.join(" ");
        run_turn(
            &conn, &session_id, &prompt, &system_prompt, &model,
            &*provider, &api_key, &base_url, &tools, &tool_defs,
            &tool_ctx, cli.thinking.as_deref(),
        )
        .await?;
    }

    // ── Main loop ──
    loop {
        // Status bar
        print_status(&conn, &session_id, &model, cwd_str);

        // Editor separator
        let w = crossterm::terminal::size().map(|(w, _)| w as usize).unwrap_or(80);
        println!("{}", style_separator(w));

        // Read input
        let input = match editor.read_line() {
            Some(text) => text,
            None => break,
        };

        let input = input.trim().to_string();
        if input.is_empty() {
            continue;
        }

        // Bash shortcut
        if input.starts_with('!') {
            handle_bash(&input, &cwd);
            continue;
        }

        // Slash commands
        if input.starts_with('/') {
            let done = handle_slash(&input, &conn, &session_id, cwd_str).await?;
            if done {
                break;
            }
            continue;
        }

        // Send to agent
        run_turn(
            &conn, &session_id, &input, &system_prompt, &model,
            &*provider, &api_key, &base_url, &tools, &tool_defs,
            &tool_ctx, cli.thinking.as_deref(),
        )
        .await?;
    }

    println!("\n {}\n", "Goodbye!".with(Color::DarkGrey));
    Ok(())
}

// ── Display a stored message (for session restore) ──────────────────

fn display_message(msg: &AgentMessage, model_id: &str) {
    match msg {
        AgentMessage::User(u) => {
            println!(" {}", style_role_user());
            for block in &u.content {
                if let ContentBlock::Text { text } = block {
                    for line in text.lines() {
                        println!("  {line}");
                    }
                }
            }
            println!();
        }
        AgentMessage::Assistant(a) => {
            println!(" {}", style_role_assistant(model_id));
            for block in &a.content {
                match block {
                    AssistantContent::Text { text } => {
                        for line in text.lines() {
                            println!("  {line}");
                        }
                    }
                    AssistantContent::Thinking { .. } => {
                        println!("  {}", style_dim("[thinking]"));
                    }
                    AssistantContent::ToolCall { name, .. } => {
                        println!("{}", style_tool_call(name));
                    }
                }
            }
            println!();
        }
        AgentMessage::ToolResult(t) => {
            let status = if t.is_error { style_tool_err() } else { style_tool_ok() };
            println!("  {} {} result:", status, t.tool_name.clone().with(Color::Cyan));
            for block in &t.content {
                if let ContentBlock::Text { text } = block {
                    for line in text.lines().take(5) {
                        println!("    {}", style_dim(line));
                    }
                    let total = text.lines().count();
                    if total > 5 {
                        println!("    {}", style_dim(&format!("[{} more lines]", total - 5)));
                    }
                }
            }
            println!();
        }
        AgentMessage::CompactionSummary(c) => {
            println!(
                " {} {}",
                "[c]".with(Color::DarkGrey),
                format!("[compaction: {} tokens summarized]", c.tokens_before).with(Color::DarkGrey),
            );
            println!();
        }
        AgentMessage::BranchSummary(b) => {
            println!(
                " {} {}",
                "[b]".with(Color::DarkGrey),
                format!("[branch summary from {}]", b.from_id).with(Color::DarkGrey),
            );
            println!();
        }
        _ => {}
    }
}

// ── Print status bar ────────────────────────────────────────────────

fn print_status(conn: &rusqlite::Connection, session_id: &str, model: &bb_provider::registry::Model, cwd: &str) {
    let w = crossterm::terminal::size().map(|(w, _)| w as usize).unwrap_or(80);
    let ctx = context::build_context(conn, session_id).ok();
    let tokens = ctx.as_ref().map(|c| {
        c.messages
            .iter()
            .map(|m| serde_json::to_string(m).unwrap_or_default().len() as u64 / 4)
            .sum::<u64>()
    }).unwrap_or(0);

    let footer = status::render_footer(
        &status::FooterData {
            model_name: model.name.clone(),
            provider: model.provider.clone(),
            thinking: "medium".into(),
            input_tokens: 0,
            output_tokens: 0,
            cost: 0.0,
            context_tokens: tokens,
            context_window: model.context_window,
            cwd: cwd.to_string(),
        },
        w,
    );
    println!("{footer}");
}

// ── Agent turn with streaming ───────────────────────────────────────

async fn run_turn(
    conn: &rusqlite::Connection,
    session_id: &str,
    prompt: &str,
    system_prompt: &str,
    model: &bb_provider::registry::Model,
    provider: &dyn Provider,
    api_key: &str,
    base_url: &str,
    tools: &[Box<dyn Tool>],
    tool_defs: &[serde_json::Value],
    tool_ctx: &ToolContext,
    thinking: Option<&str>,
) -> Result<()> {
    // Append + display user message
    let user_entry = SessionEntry::Message {
        base: EntryBase {
            id: EntryId::generate(),
            parent_id: get_leaf(conn, session_id),
            timestamp: Utc::now(),
        },
        message: AgentMessage::User(UserMessage {
            content: vec![ContentBlock::Text { text: prompt.to_string() }],
            timestamp: Utc::now().timestamp_millis(),
        }),
    };
    store::append_entry(conn, session_id, &user_entry)?;

    println!("\n {}", style_role_user());
    println!("  {}\n", prompt);

    // Agent loop
    loop {
        let ctx = context::build_context(conn, session_id)?;
        let provider_messages = crate::run::messages_to_provider(&ctx.messages);

        let request = CompletionRequest {
            system_prompt: system_prompt.to_string(),
            messages: provider_messages,
            tools: tool_defs.to_vec(),
            model: model.id.clone(),
            max_tokens: Some(model.max_tokens as u32),
            stream: true,
            thinking: thinking.map(|s| s.to_string()),
        };
        let options = RequestOptions {
            api_key: api_key.to_string(),
            base_url: base_url.to_string(),
            headers: std::collections::HashMap::new(),
            cancel: CancellationToken::new(),
        };

        // Stream
        let (tx, mut rx) = mpsc::unbounded_channel();

        println!(" {}", style_role_assistant(&model.id));

        let stream_result = provider.stream(request, options, tx).await;
        if let Err(e) = stream_result {
            println!("  {}", style_error(&format!("Error: {e}")));
            break;
        }

        let mut all_events = Vec::new();
        let mut text_started = false;

        while let Some(event) = rx.recv().await {
            match &event {
                StreamEvent::TextDelta { text } => {
                    if !text_started {
                        text_started = true;
                        print!("  ");
                    }
                    print!("{text}");
                    std::io::stdout().flush().ok();
                }
                StreamEvent::ThinkingDelta { .. } => {
                    if !text_started {
                        print!("  {}", style_dim("[thinking...] "));
                        std::io::stdout().flush().ok();
                    }
                }
                StreamEvent::ToolCallStart { name, id } => {
                    if text_started {
                        println!();
                        text_started = false;
                    }
                    // Tool call name shown, args will come via deltas
                    print!("{}", style_tool_call(name));
                }
                StreamEvent::ToolCallDelta { arguments_delta, .. } => {
                    // We could show partial args, but for now just collect
                }
                StreamEvent::ToolCallEnd { .. } => {
                    println!(); // newline after tool call line
                }
                StreamEvent::Error { message } => {
                    if text_started {
                        println!();
                    }
                    println!("  {}", style_error(&format!("Error: {message}")));
                }
                _ => {}
            }
            all_events.push(event);
        }

        if text_started {
            println!();
        }

        // Collect response
        let collected = bb_provider::streaming::CollectedResponse::from_events(&all_events);

        // Re-render the text with markdown formatting
        if !collected.text.is_empty() {
            // Move cursor up to overwrite the raw streamed text
            let raw_line_count = collected.text.lines().count() + 1; // +1 for indent
            // Clear and re-render with markdown
            print!("\x1b[{}A", raw_line_count); // move up
            for _ in 0..raw_line_count {
                println!("\x1b[2K"); // clear each line
            }
            print!("\x1b[{}A", raw_line_count); // move up again

            let term_width = crossterm::terminal::size().map(|(w, _)| w).unwrap_or(80);
            let md_width = term_width.saturating_sub(4); // 2 indent each side
            let mut renderer = MarkdownRenderer::new(&collected.text);
            let md_lines = renderer.render(md_width);
            for line in &md_lines {
                println!("  {line}");
            }
        }
        println!();

        // Build assistant message
        let mut content = Vec::new();
        if !collected.thinking.is_empty() {
            content.push(AssistantContent::Thinking { thinking: collected.thinking });
        }
        if !collected.text.is_empty() {
            content.push(AssistantContent::Text { text: collected.text });
        }
        for tc in &collected.tool_calls {
            let args: serde_json::Value =
                serde_json::from_str(&tc.arguments).unwrap_or(serde_json::json!({}));
            content.push(AssistantContent::ToolCall {
                id: tc.id.clone(),
                name: tc.name.clone(),
                arguments: args,
            });
        }

        let asst_entry = SessionEntry::Message {
            base: EntryBase {
                id: EntryId::generate(),
                parent_id: get_leaf(conn, session_id),
                timestamp: Utc::now(),
            },
            message: AgentMessage::Assistant(AssistantMessage {
                content,
                provider: model.provider.clone(),
                model: model.id.clone(),
                usage: Usage {
                    input: collected.input_tokens,
                    output: collected.output_tokens,
                    ..Default::default()
                },
                stop_reason: if collected.tool_calls.is_empty() { StopReason::Stop } else { StopReason::ToolUse },
                error_message: None,
                timestamp: Utc::now().timestamp_millis(),
            }),
        };
        store::append_entry(conn, session_id, &asst_entry)?;

        if collected.tool_calls.is_empty() {
            break;
        }

        // Execute tools
        let cancel = CancellationToken::new();
        for tc in &collected.tool_calls {
            let args: serde_json::Value =
                serde_json::from_str(&tc.arguments).unwrap_or(serde_json::json!({}));

            print!("{}", style_tool_running(&tc.name));
            std::io::stdout().flush().ok();

            let tool = tools.iter().find(|t| t.name() == tc.name);
            let result = match tool {
                Some(t) => t.execute(args, tool_ctx, cancel.clone()).await,
                None => Err(bb_core::error::BbError::Tool(format!("Unknown tool: {}", tc.name))),
            };

            let (content, is_error) = match result {
                Ok(r) => {
                    println!("{}", style_tool_ok());
                    for block in &r.content {
                        if let ContentBlock::Text { text } = block {
                            for line in text.lines().take(5) {
                                println!("    {}", style_dim(line));
                            }
                            let total = text.lines().count();
                            if total > 5 {
                                println!("    {}", style_dim(&format!("[{} more lines]", total - 5)));
                            }
                        }
                    }
                    (r.content, r.is_error)
                }
                Err(e) => {
                    println!("{}", style_tool_err());
                    let msg = format!("Error: {e}");
                    println!("    {}", style_error(&msg));
                    (vec![ContentBlock::Text { text: msg }], true)
                }
            };

            let tr_entry = SessionEntry::Message {
                base: EntryBase {
                    id: EntryId::generate(),
                    parent_id: get_leaf(conn, session_id),
                    timestamp: Utc::now(),
                },
                message: AgentMessage::ToolResult(ToolResultMessage {
                    tool_call_id: tc.id.clone(),
                    tool_name: tc.name.clone(),
                    content,
                    details: None,
                    is_error,
                    timestamp: Utc::now().timestamp_millis(),
                }),
            };
            store::append_entry(conn, session_id, &tr_entry)?;
        }

        println!();
    }

    Ok(())
}

// ── Bash shortcut ───────────────────────────────────────────────────

fn handle_bash(input: &str, cwd: &std::path::Path) {
    let cmd = input
        .strip_prefix("!!")
        .or_else(|| input.strip_prefix('!'))
        .unwrap_or("")
        .trim();
    if cmd.is_empty() {
        return;
    }
    println!("  {} {}", "$".with(Color::DarkGrey), cmd);
    match std::process::Command::new("bash")
        .arg("-c")
        .arg(cmd)
        .current_dir(cwd)
        .output()
    {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !stdout.is_empty() {
                print!("{stdout}");
            }
            if !stderr.is_empty() {
                eprint!("{stderr}");
            }
        }
        Err(e) => {
            println!("  {}", style_error(&format!("bash error: {e}")));
        }
    }
    println!();
}

// ── Slash command handling ──────────────────────────────────────────

async fn handle_slash(
    input: &str,
    conn: &rusqlite::Connection,
    session_id: &str,
    cwd_str: &str,
) -> Result<bool> {
    match slash::handle_slash_command(input) {
        SlashResult::Exit => return Ok(true),
        SlashResult::Handled => {}
        SlashResult::NewSession => {
            println!("  Start a new `bb` to get a fresh session.");
        }
        SlashResult::Compact(_) => {
            println!("  {}", style_dim("(manual compaction not yet wired)"));
        }
        SlashResult::ModelSelect(_) => {
            crate::models::list_models(None);
        }
        SlashResult::Resume => {
            let sessions = store::list_sessions(conn, cwd_str)?;
            if sessions.is_empty() {
                println!("  No sessions to resume.");
            } else {
                println!("  {}", "Recent sessions:".bold());
                for (i, s) in sessions.iter().take(10).enumerate() {
                    let name = s.name.as_deref().unwrap_or("(unnamed)");
                    println!(
                        "  {}. {} {} ({} entries)",
                        i + 1,
                        &s.session_id[..8],
                        name.with(Color::Cyan),
                        s.entry_count,
                    );
                }
            }
        }
        SlashResult::Tree | SlashResult::Fork => {
            println!("  {}", style_dim("(not yet implemented)"));
        }
        SlashResult::Login => {
            login::handle_login(None).await?;
        }
        SlashResult::Logout => {
            login::handle_logout(None).await?;
        }
        SlashResult::SetName(name) => {
            println!("  Session named: {}", name.with(Color::Cyan));
        }
        SlashResult::SessionInfo => {
            if let Ok(Some(session)) = store::get_session(conn, session_id) {
                println!("  Session:  {}", &session.session_id[..8]);
                println!("  Name:     {}", session.name.unwrap_or("(unnamed)".into()));
                println!("  CWD:      {}", session.cwd);
                println!("  Entries:  {}", session.entry_count);
                println!("  Updated:  {}", session.updated_at);
            }
        }
        SlashResult::NotCommand => {}
    }
    Ok(false)
}

fn get_leaf(conn: &rusqlite::Connection, session_id: &str) -> Option<EntryId> {
    store::get_session(conn, session_id)
        .ok()
        .flatten()
        .and_then(|s| s.leaf_id.map(EntryId))
}
