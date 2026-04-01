//! Interactive mode — scrollback-based TUI.
//!
//! Design: messages are printed to the scrollback buffer (like a normal CLI).
//! Only the editor at the bottom uses raw mode for input. Streaming output
//! is printed in real-time as tokens arrive.

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
use bb_tui::chat;
use bb_tui::editor::Editor;
use bb_tui::status;
use chrono::Utc;
use crossterm::style::{Color, Stylize};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::login;
use crate::slash::{self, SlashResult};
use crate::Cli;

/// Run bb in interactive mode.
pub async fn run_interactive(cli: Cli) -> Result<()> {
    let cwd = std::fs::canonicalize(cli.cwd.as_deref().unwrap_or("."))?;

    // Setup directories
    let global_dir = config::global_dir();
    std::fs::create_dir_all(&global_dir)?;
    let artifacts_dir = global_dir.join("artifacts");
    std::fs::create_dir_all(&artifacts_dir)?;

    // Open session database
    let db_path = global_dir.join("sessions.db");
    let conn = store::open_db(&db_path)?;

    // Session
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

    // Parse model
    let (provider_name, model_id, _thinking) = crate::run::parse_model_arg(
        cli.provider.as_deref(),
        cli.model.as_deref(),
    );

    // Load AGENTS.md
    let agents_md = crate::run::load_agents_md(&cwd);
    let system_prompt = agent::build_system_prompt(
        cli.system_prompt.as_deref().unwrap_or(DEFAULT_SYSTEM_PROMPT),
        agents_md.as_deref(),
    );

    // Model registry + resolve
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

    // API key
    let api_key = cli.api_key.clone().unwrap_or_else(|| {
        login::resolve_api_key(&provider_name).unwrap_or_default()
    });
    if api_key.is_empty() {
        eprintln!(
            "{}",
            format!(
                "Warning: No API key for '{}'. Run `bb login` or set env var.",
                provider_name
            )
            .with(Color::Yellow)
        );
    }
    let base_url = model
        .base_url
        .clone()
        .unwrap_or_else(|| "https://api.openai.com/v1".into());

    // Tools
    let tools = builtin_tools();
    let tool_ctx = ToolContext {
        cwd: cwd.clone(),
        artifacts_dir: artifacts_dir.clone(),
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

    // Provider
    let provider: Box<dyn Provider> = match model.api {
        ApiType::AnthropicMessages => Box::new(AnthropicProvider::new()),
        _ => Box::new(OpenAiProvider::new()),
    };

    // Editor
    let mut editor = Editor::new("> ");

    // ── Banner ──
    println!(
        "\n {} v{}",
        "bb-agent".with(Color::Cyan).bold(),
        env!("CARGO_PKG_VERSION"),
    );
    println!(
        " {} {} | {} {}K context",
        "model:".with(Color::DarkGrey),
        model.name.clone().with(Color::Cyan),
        "window:".with(Color::DarkGrey),
        model.context_window / 1000,
    );
    println!(
        " {} to submit, {} to exit\n",
        "Enter".with(Color::DarkGrey),
        "Ctrl+D".with(Color::DarkGrey),
    );

    // If --continue, show restored messages
    if cli.r#continue {
        if let Ok(ctx) = context::build_context(&conn, &session_id) {
            for msg in &ctx.messages {
                let lines = chat::render_message(msg);
                for line in &lines {
                    println!("{line}");
                }
            }
        }
    }

    // If initial prompt provided, run it
    if !cli.messages.is_empty() {
        let prompt = cli.messages.join(" ");
        run_turn(
            &conn,
            &session_id,
            &prompt,
            &system_prompt,
            &model,
            &*provider,
            &api_key,
            &base_url,
            &tools,
            &tool_defs,
            &tool_ctx,
            cli.thinking.as_deref(),
        )
        .await?;
    }

    // ── Main loop ──
    loop {
        // Show status bar before editor
        let ctx_usage = context::build_context(&conn, &session_id).ok();
        let total_tokens = ctx_usage.as_ref().map(|c| {
            c.messages
                .iter()
                .map(|m| {
                    let s = serde_json::to_string(m).unwrap_or_default();
                    (s.len() as u64) / 4
                })
                .sum::<u64>()
        });
        let status_line = status::render_status(
            Some(&model.name),
            total_tokens,
            Some(model.context_window),
        );
        if !status_line.is_empty() {
            println!("{status_line}");
        }

        // Read input (blocking, raw mode inside)
        let input = match editor.read_line() {
            Some(text) => text,
            None => break, // Ctrl+D / Ctrl+C on empty
        };

        let input = input.trim().to_string();
        if input.is_empty() {
            continue;
        }

        // ── Bash shortcut ──
        if input.starts_with('!') {
            let cmd = input.strip_prefix("!!").or_else(|| input.strip_prefix('!'));
            if let Some(cmd) = cmd {
                let cmd = cmd.trim();
                if !cmd.is_empty() {
                    println!("  {} {}", "$".with(Color::DarkGrey), cmd);
                    let output = std::process::Command::new("bash")
                        .arg("-c")
                        .arg(cmd)
                        .current_dir(&cwd)
                        .output()?;
                    print!("{}", String::from_utf8_lossy(&output.stdout));
                    if !output.stderr.is_empty() {
                        eprint!("{}", String::from_utf8_lossy(&output.stderr));
                    }
                    println!();
                }
            }
            continue;
        }

        // ── Slash commands ──
        if input.starts_with('/') {
            match slash::handle_slash_command(&input) {
                SlashResult::Exit => break,
                SlashResult::Handled => continue,
                SlashResult::NewSession => {
                    // Can't rebind conn easily, just inform
                    println!("  Start a new `bb` session to get a fresh context.");
                    continue;
                }
                SlashResult::Compact(_instructions) => {
                    println!("  {} Compacting...", "📦".with(Color::DarkGrey));
                    // TODO: wire to real compaction
                    println!("  (manual compaction not yet wired)");
                    continue;
                }
                SlashResult::ModelSelect(_search) => {
                    crate::models::list_models(None);
                    continue;
                }
                SlashResult::Resume => {
                    let sessions = store::list_sessions(&conn, cwd_str)?;
                    if sessions.is_empty() {
                        println!("  No sessions to resume.");
                    } else {
                        for (i, s) in sessions.iter().take(10).enumerate() {
                            let name = s.name.as_deref().unwrap_or("(unnamed)");
                            println!(
                                "  {}. {} {} ({} entries)",
                                i + 1,
                                &s.session_id[..8],
                                name,
                                s.entry_count
                            );
                        }
                    }
                    continue;
                }
                SlashResult::Tree | SlashResult::Fork => {
                    println!("  (not yet implemented in interactive mode)");
                    continue;
                }
                SlashResult::Login => {
                    login::handle_login(None).await?;
                    continue;
                }
                SlashResult::Logout => {
                    login::handle_logout(None).await?;
                    continue;
                }
                SlashResult::SetName(name) => {
                    println!("  Session named: {name}");
                    continue;
                }
                SlashResult::SessionInfo => {
                    if let Some(session) = store::get_session(&conn, &session_id)? {
                        println!("  Session: {}", &session.session_id[..8]);
                        println!("  Name:    {}", session.name.unwrap_or("(unnamed)".into()));
                        println!("  CWD:     {}", session.cwd);
                        println!("  Entries: {}", session.entry_count);
                    }
                    continue;
                }
                SlashResult::NotCommand => {
                    // Not a command — fall through and send to LLM
                }
            }
        }

        // ── Send to agent ──
        run_turn(
            &conn,
            &session_id,
            &input,
            &system_prompt,
            &model,
            &*provider,
            &api_key,
            &base_url,
            &tools,
            &tool_defs,
            &tool_ctx,
            cli.thinking.as_deref(),
        )
        .await?;
    }

    println!("\n  {}\n", "Goodbye!".with(Color::DarkGrey));
    Ok(())
}

/// Run one user prompt through the full agent loop with streaming display.
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
    // ── Append + display user message ──
    let user_entry = SessionEntry::Message {
        base: EntryBase {
            id: EntryId::generate(),
            parent_id: get_leaf(conn, session_id),
            timestamp: Utc::now(),
        },
        message: AgentMessage::User(UserMessage {
            content: vec![ContentBlock::Text {
                text: prompt.to_string(),
            }],
            timestamp: Utc::now().timestamp_millis(),
        }),
    };
    store::append_entry(conn, session_id, &user_entry)?;

    // Display user message
    println!(
        "\n {} {}",
        "You".with(Color::Blue).bold(),
        "".with(Color::DarkGrey),
    );
    println!("  {}\n", prompt);

    // ── Agent loop ──
    loop {
        // Build context
        let ctx = context::build_context(conn, session_id)?;
        let provider_messages = crate::run::messages_to_provider(&ctx.messages);

        // Build request
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

        // Stream response with real-time display
        let (tx, mut rx) = mpsc::unbounded_channel();

        // Print assistant header
        print!(
            " {}{}",
            "Assistant".with(Color::Green).bold(),
            format!(" ({})", model.id).with(Color::DarkGrey),
        );
        println!();

        // Spawn streaming request
        let stream_result = provider.stream(request, options, tx).await;
        if let Err(e) = stream_result {
            println!(
                "  {}",
                format!("Error: {e}").with(Color::Red)
            );
            break;
        }

        // Collect events while displaying
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
                        print!(
                            "  {}",
                            "[thinking...] ".with(Color::DarkGrey),
                        );
                        std::io::stdout().flush().ok();
                    }
                }
                StreamEvent::ToolCallStart { name, .. } => {
                    if text_started {
                        println!();
                    }
                    print!(
                        "  {} {}",
                        "⚡".with(Color::Yellow),
                        name.clone().bold(),
                    );
                    std::io::stdout().flush().ok();
                }
                StreamEvent::ToolCallEnd { .. } => {
                    println!();
                }
                StreamEvent::Error { message } => {
                    println!();
                    println!(
                        "  {}",
                        format!("Error: {message}").with(Color::Red)
                    );
                }
                StreamEvent::Done | StreamEvent::Usage(_) | StreamEvent::ToolCallDelta { .. } => {}
            }
            all_events.push(event);
        }

        // Ensure newline after output
        if text_started {
            println!();
        }
        println!();

        // Collect final response
        let collected = bb_provider::streaming::CollectedResponse::from_events(&all_events);

        // Build and store assistant message
        let mut assistant_content = Vec::new();
        if !collected.thinking.is_empty() {
            assistant_content.push(AssistantContent::Thinking {
                thinking: collected.thinking,
            });
        }
        if !collected.text.is_empty() {
            assistant_content.push(AssistantContent::Text {
                text: collected.text,
            });
        }
        for tc in &collected.tool_calls {
            let args: serde_json::Value =
                serde_json::from_str(&tc.arguments).unwrap_or(serde_json::json!({}));
            assistant_content.push(AssistantContent::ToolCall {
                id: tc.id.clone(),
                name: tc.name.clone(),
                arguments: args,
            });
        }

        let assistant_msg = AgentMessage::Assistant(AssistantMessage {
            content: assistant_content,
            provider: model.provider.clone(),
            model: model.id.clone(),
            usage: Usage {
                input: collected.input_tokens,
                output: collected.output_tokens,
                ..Default::default()
            },
            stop_reason: if collected.tool_calls.is_empty() {
                StopReason::Stop
            } else {
                StopReason::ToolUse
            },
            error_message: None,
            timestamp: Utc::now().timestamp_millis(),
        });

        let asst_entry = SessionEntry::Message {
            base: EntryBase {
                id: EntryId::generate(),
                parent_id: get_leaf(conn, session_id),
                timestamp: Utc::now(),
            },
            message: assistant_msg,
        };
        store::append_entry(conn, session_id, &asst_entry)?;

        // No tool calls → done
        if collected.tool_calls.is_empty() {
            break;
        }

        // ── Execute tool calls ──
        let cancel = CancellationToken::new();
        for tc in &collected.tool_calls {
            let args: serde_json::Value =
                serde_json::from_str(&tc.arguments).unwrap_or(serde_json::json!({}));

            print!(
                "  {} {} ",
                "⏳",
                tc.name.clone().with(Color::Cyan),
            );
            std::io::stdout().flush().ok();

            let tool = tools.iter().find(|t| t.name() == tc.name);
            let result = match tool {
                Some(t) => t.execute(args, tool_ctx, cancel.clone()).await,
                None => Err(bb_core::error::BbError::Tool(format!(
                    "Unknown tool: {}",
                    tc.name
                ))),
            };

            let (content, is_error) = match result {
                Ok(r) => {
                    println!("{}", "✓".with(Color::Green));
                    // Show brief preview
                    for block in &r.content {
                        if let ContentBlock::Text { text } = block {
                            let preview: Vec<&str> = text.lines().take(5).collect();
                            for line in &preview {
                                println!(
                                    "    {}",
                                    line.with(Color::DarkGrey)
                                );
                            }
                            let total = text.lines().count();
                            if total > 5 {
                                println!(
                                    "    {}",
                                    format!("[{} more lines]", total - 5)
                                        .with(Color::DarkGrey)
                                );
                            }
                        }
                    }
                    (r.content, r.is_error)
                }
                Err(e) => {
                    println!("{}", "✗".with(Color::Red));
                    let msg = format!("Error: {e}");
                    println!("    {}", msg.clone().with(Color::Red));
                    (
                        vec![ContentBlock::Text { text: msg }],
                        true,
                    )
                }
            };

            // Store tool result
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
        // Continue loop — next LLM call will see tool results
    }

    Ok(())
}

fn get_leaf(conn: &rusqlite::Connection, session_id: &str) -> Option<EntryId> {
    store::get_session(conn, session_id)
        .ok()
        .flatten()
        .and_then(|s| s.leaf_id.map(EntryId))
}
