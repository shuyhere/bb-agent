use anyhow::Result;

use bb_core::agent::{self, DEFAULT_SYSTEM_PROMPT};
use bb_core::config;
use bb_core::settings::Settings;
use bb_core::types::*;
use bb_hooks::EventBus;
use bb_provider::anthropic::AnthropicProvider;
use bb_provider::openai::OpenAiProvider;
use bb_provider::registry::{ApiType, ModelRegistry};
use bb_provider::streaming::CollectedResponse;
use bb_provider::{CompletionRequest, Provider, RequestOptions, StreamEvent};
use bb_session::{context, store};
use bb_tools::{builtin_tools, Tool, ToolContext};
use bb_tui::app::App;
use chrono::Utc;
use tokio_util::sync::CancellationToken;

use crate::login;
use crate::slash::{self, SlashResult};
use crate::Cli;

pub async fn run_print_mode(cli: Cli) -> Result<()> {
    let cwd = std::fs::canonicalize(cli.cwd.as_deref().unwrap_or("."))?;

    // Setup directories
    let global_dir = config::global_dir();
    std::fs::create_dir_all(&global_dir)?;
    let artifacts_dir = global_dir.join("artifacts");
    std::fs::create_dir_all(&artifacts_dir)?;

    // Open session database
    let db_path = global_dir.join("sessions.db");
    let conn = store::open_db(&db_path)?;

    // Session management
    let session_id = if cli.r#continue {
        // Continue most recent session for this cwd
        let sessions = store::list_sessions(&conn, cwd.to_str().unwrap_or("."))?;
        match sessions.first() {
            Some(s) => {
                tracing::info!("Continuing session {}", s.session_id);
                s.session_id.clone()
            }
            None => store::create_session(&conn, cwd.to_str().unwrap_or("."))?,
        }
    } else if cli.no_session {
        // Ephemeral: use in-memory (just create but don't persist path)
        store::create_session(&conn, cwd.to_str().unwrap_or("."))?
    } else {
        store::create_session(&conn, cwd.to_str().unwrap_or("."))?
    };

    // Load layered settings
    let settings = Settings::load_merged(&cwd);

    // Parse --model (supports "provider/model" and "model:thinking")
    // CLI flags override settings defaults
    let model_input = cli.model.as_deref()
        .or(settings.default_model.as_deref());
    let provider_input = cli.provider.as_deref()
        .or(settings.default_provider.as_deref());

    let (provider_name, model_id, thinking_override) = parse_model_arg(
        provider_input,
        model_input,
    );

    // Load AGENTS.md
    let agents_md = load_agents_md(&cwd);
    let base_prompt = cli
        .system_prompt
        .as_deref()
        .unwrap_or(DEFAULT_SYSTEM_PROMPT);
    let system_prompt = match &cli.append_system_prompt {
        Some(append) => agent::build_system_prompt(base_prompt, Some(append)),
        None => agent::build_system_prompt(base_prompt, agents_md.as_deref()),
    };

    // Model registry (builtins + custom models from settings)
    let mut registry = ModelRegistry::new();
    registry.load_custom_models(&settings);

    // Try exact match first, then fuzzy
    let model = registry
        .find(&provider_name, &model_id)
        .cloned()
        .or_else(|| registry.find_fuzzy(&model_id, Some(&provider_name)).cloned())
        .or_else(|| registry.find_fuzzy(&model_id, None).cloned())
        .unwrap_or_else(|| {
            bb_provider::registry::Model {
                id: model_id.clone(),
                name: model_id.clone(),
                provider: provider_name.clone(),
                api: bb_provider::registry::ApiType::OpenaiCompletions,
                context_window: 128_000,
                max_tokens: 16384,
                reasoning: false,
                base_url: None,
                cost: Default::default(),
            }
        });

    // Resolve API key
    let api_key = match &cli.api_key {
        Some(k) => k.clone(),
        None => login::resolve_api_key(&provider_name).unwrap_or_default(),
    };

    if api_key.is_empty() && !cli.print {
        eprintln!(
            "Warning: No API key for provider '{}'. Run `bb login` or set the appropriate env var.",
            provider_name
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

    // Build tool definitions for the provider
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

    // Event bus (for future plugin support)
    let _event_bus = EventBus::new();

    // Provider — select based on API type
    let provider: Box<dyn Provider> = match model.api {
        ApiType::AnthropicMessages => Box::new(AnthropicProvider::new()),
        _ => Box::new(OpenAiProvider::new()),
    };

    // TUI app
    let mut app = App::new();
    app.set_model(&model.name);

    if cli.print && !cli.messages.is_empty() {
        // Print mode: single prompt
        let prompt = cli.messages.join(" ");
        run_turn(
            &conn, &session_id, &prompt, &system_prompt, &model, &*provider,
            &api_key, &base_url, &tools, &tool_defs, &tool_ctx, &app,
        )
        .await?;
        return Ok(());
    }

    // Interactive mode
    app.print_banner();
    app.display_status(None, Some(model.context_window));

    // If initial messages provided, run them first
    if !cli.messages.is_empty() {
        let prompt = cli.messages.join(" ");
        run_turn(
            &conn, &session_id, &prompt, &system_prompt, &model, &*provider,
            &api_key, &base_url, &tools, &tool_defs, &tool_ctx, &app,
        )
        .await?;
    }

    // Main interactive loop
    loop {
        let input = match app.read_input() {
            Some(input) => input,
            None => break, // Ctrl+C / Ctrl+D
        };

        let input = input.trim().to_string();
        if input.is_empty() {
            continue;
        }

        // Handle ! prefix for direct bash
        if input.starts_with('!') {
            let cmd = if input.starts_with("!!") {
                &input[2..]
            } else {
                &input[1..]
            };
            let cmd = cmd.trim();
            if !cmd.is_empty() {
                println!("$ {cmd}");
                let output = std::process::Command::new("bash")
                    .arg("-c")
                    .arg(cmd)
                    .current_dir(&cwd)
                    .output()?;
                print!("{}", String::from_utf8_lossy(&output.stdout));
                if !output.stderr.is_empty() {
                    eprint!("{}", String::from_utf8_lossy(&output.stderr));
                }
            }
            continue;
        }

        // Handle slash commands
        if input.starts_with('/') {
            match slash::handle_slash_command(&input) {
                SlashResult::Exit => break,
                SlashResult::Handled => continue,
                SlashResult::NewSession => {
                    // TODO: create new session, reset state
                    continue;
                }
                SlashResult::Compact(_instructions) => {
                    println!("Compaction not yet implemented in interactive mode.");
                    continue;
                }
                SlashResult::ModelSelect(search) => {
                    crate::models::list_models(search.as_deref());
                    continue;
                }
                SlashResult::Resume => {
                    let sessions = store::list_sessions(&conn, cwd.to_str().unwrap_or("."))?;
                    if sessions.is_empty() {
                        println!("No sessions to resume.");
                    } else {
                        println!("Recent sessions:");
                        for (i, s) in sessions.iter().take(10).enumerate() {
                            let name = s.name.as_deref().unwrap_or("(unnamed)");
                            println!("  {}. {} {} ({} entries)", i + 1, &s.session_id[..8], name, s.entry_count);
                        }
                    }
                    continue;
                }
                SlashResult::Tree => {
                    println!("Tree navigation not yet implemented.");
                    continue;
                }
                SlashResult::Fork => {
                    println!("Fork not yet implemented.");
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
                    println!("Session named: {name}");
                    // TODO: persist via session_info entry
                    continue;
                }
                SlashResult::NotCommand => {} // fall through to LLM
            }
        }

        run_turn(
            &conn, &session_id, &input, &system_prompt, &model, &*provider,
            &api_key, &base_url, &tools, &tool_defs, &tool_ctx, &app,
        )
        .await?;
    }

    println!("\nGoodbye!");
    Ok(())
}

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
    app: &App,
) -> Result<()> {
    use crossterm::style::{Color, Stylize};
    use std::io::Write;
    use tokio::sync::mpsc;

    // Append user message
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
    if let SessionEntry::Message { message, .. } = &user_entry {
        app.display_message(message);
    }

    // Agent loop
    loop {
        let ctx = context::build_context(conn, session_id)?;
        let provider_messages = messages_to_provider(&ctx.messages);

        let request = CompletionRequest {
            system_prompt: system_prompt.to_string(),
            messages: provider_messages,
            tools: tool_defs.to_vec(),
            model: model.id.clone(),
            max_tokens: Some(model.max_tokens as u32),
            stream: true,
        };

        let options = RequestOptions {
            api_key: api_key.to_string(),
            base_url: base_url.to_string(),
            headers: std::collections::HashMap::new(),
            cancel: CancellationToken::new(),
        };

        // Stream with real-time display
        let (tx, mut rx) = mpsc::unbounded_channel();
        let model_name = model.name.clone();

        // Print assistant header
        print!(
            "{}{} ",
            "Assistant".bold().with(Color::Green),
            format!(" ({})", model.id).with(Color::DarkGrey),
        );
        std::io::stdout().flush().ok();

        // Spawn the streaming request
        let stream_result = provider.stream(request, options, tx).await;
        if let Err(e) = stream_result {
            println!();
            eprintln!("{}", format!("Provider error: {e}").with(Color::Red));
            break;
        }

        // Collect events while streaming text to terminal
        let mut all_events = Vec::new();
        let mut started_text = false;
        let mut started_tool = false;

        println!(); // newline after header
        while let Some(event) = rx.recv().await {
            match &event {
                StreamEvent::TextDelta { text } => {
                    if !started_text {
                        started_text = true;
                        print!("  ");
                    }
                    // Print each delta in real-time
                    print!("{text}");
                    std::io::stdout().flush().ok();
                }
                StreamEvent::ThinkingDelta { text } => {
                    // Show thinking indicator (dimmed)
                    if !started_text {
                        started_text = true;
                        print!("  {}", "[thinking] ".with(Color::DarkGrey));
                    }
                }
                StreamEvent::ToolCallStart { name, .. } => {
                    if started_text {
                        println!();
                    }
                    print!(
                        "  {} {}",
                        "⚡".with(Color::Yellow),
                        name.clone().bold(),
                    );
                    std::io::stdout().flush().ok();
                    started_tool = true;
                }
                StreamEvent::ToolCallEnd { .. } => {
                    if started_tool {
                        println!();
                        started_tool = false;
                    }
                }
                StreamEvent::Done => {}
                StreamEvent::Error { message } => {
                    println!();
                    eprintln!("{}", format!("Stream error: {message}").with(Color::Red));
                }
                _ => {}
            }
            all_events.push(event);
        }

        // Ensure newline after streaming output
        if started_text || started_tool {
            println!();
        }
        println!();

        // Collect final response
        let collected = CollectedResponse::from_events(&all_events);

        // Build assistant message for session storage
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

        if collected.tool_calls.is_empty() {
            break;
        }

        // Execute tool calls
        let cancel = CancellationToken::new();
        for tc in &collected.tool_calls {
            let args: serde_json::Value =
                serde_json::from_str(&tc.arguments).unwrap_or(serde_json::json!({}));

            print!(
                "  {} {} ",
                if true { "⏳" } else { "⚡" },
                tc.name.clone().with(Color::Cyan),
            );
            std::io::stdout().flush().ok();

            let tool = tools.iter().find(|t| t.name() == tc.name);
            let result = match tool {
                Some(t) => t.execute(args, tool_ctx, cancel.clone()).await,
                None => Err(bb_core::error::BbError::Tool(format!("Unknown tool: {}", tc.name))),
            };

            let (content, is_error) = match result {
                Ok(r) => {
                    println!("{}", "✓".with(Color::Green));
                    // Show brief result preview
                    for block in &r.content {
                        if let ContentBlock::Text { text } = block {
                            let preview: Vec<&str> = text.lines().take(5).collect();
                            for line in &preview {
                                println!("    {}", line.with(Color::DarkGrey));
                            }
                            let total = text.lines().count();
                            if total > 5 {
                                println!("    {}", format!("[{} more lines]", total - 5).with(Color::DarkGrey));
                            }
                        }
                    }
                    (r.content, r.is_error)
                }
                Err(e) => {
                    println!("{}", "✗".with(Color::Red));
                    let msg = format!("Error: {e}");
                    println!("    {}", msg.clone().with(Color::Red));
                    (vec![ContentBlock::Text { text: msg }], true)
                }
            };

            let tool_result_msg = AgentMessage::ToolResult(ToolResultMessage {
                tool_call_id: tc.id.clone(),
                tool_name: tc.name.clone(),
                content,
                details: None,
                is_error,
                timestamp: Utc::now().timestamp_millis(),
            });

            let tr_entry = SessionEntry::Message {
                base: EntryBase {
                    id: EntryId::generate(),
                    parent_id: get_leaf(conn, session_id),
                    timestamp: Utc::now(),
                },
                message: tool_result_msg,
            };
            store::append_entry(conn, session_id, &tr_entry)?;
        }

        println!();
    }

    Ok(())
}

pub(crate) fn get_leaf(conn: &rusqlite::Connection, session_id: &str) -> Option<EntryId> {
    store::get_session(conn, session_id)
        .ok()
        .flatten()
        .and_then(|s| s.leaf_id.map(EntryId))
}

/// Parse --model flag. Supports:
///   "gpt-4o"                   -> (default_provider, "gpt-4o", None)
///   "openai/gpt-4o"            -> ("openai", "gpt-4o", None)
///   "sonnet:high"              -> (default, fuzzy "sonnet", Some("high"))
///   "anthropic/sonnet:high"    -> ("anthropic", fuzzy "sonnet", Some("high"))
pub(crate) fn parse_model_arg(provider: Option<&str>, model: Option<&str>) -> (String, String, Option<String>) {
    let default_provider = provider.unwrap_or("openai").to_string();

    let model_str = match model {
        Some(m) => m,
        None => return (default_provider, "gpt-4o".to_string(), None),
    };

    // Split thinking level
    let (model_part, thinking) = if let Some(pos) = model_str.rfind(':') {
        let level = &model_str[pos + 1..];
        let valid = ["off", "low", "medium", "high", "minimal", "xhigh"];
        if valid.contains(&level) {
            (&model_str[..pos], Some(level.to_string()))
        } else {
            (model_str, None)
        }
    } else {
        (model_str, None)
    };

    // Split provider
    if let Some(pos) = model_part.find('/') {
        let prov = &model_part[..pos];
        let mid = &model_part[pos + 1..];
        (prov.to_string(), mid.to_string(), thinking)
    } else {
        (default_provider, model_part.to_string(), thinking)
    }
}

pub(crate) fn messages_to_provider(messages: &[AgentMessage]) -> Vec<serde_json::Value> {
    messages
        .iter()
        .filter_map(|msg| match msg {
            AgentMessage::User(u) => {
                let text = u.content.iter()
                    .filter_map(|c| match c { ContentBlock::Text { text } => Some(text.as_str()), _ => None })
                    .collect::<Vec<_>>()
                    .join("\n");
                Some(serde_json::json!({"role": "user", "content": text}))
            }
            AgentMessage::Assistant(a) => {
                let text = agent::extract_text(&a.content);
                let tool_calls: Vec<serde_json::Value> = a.content.iter()
                    .filter_map(|c| match c {
                        AssistantContent::ToolCall { id, name, arguments } => Some(serde_json::json!({
                            "id": id, "type": "function",
                            "function": { "name": name, "arguments": serde_json::to_string(arguments).unwrap_or_default() }
                        })),
                        _ => None,
                    })
                    .collect();
                let mut msg = serde_json::json!({"role": "assistant"});
                if !text.is_empty() { msg["content"] = serde_json::json!(text); }
                if !tool_calls.is_empty() { msg["tool_calls"] = serde_json::json!(tool_calls); }
                Some(msg)
            }
            AgentMessage::ToolResult(t) => {
                let text = t.content.iter()
                    .filter_map(|c| match c { ContentBlock::Text { text } => Some(text.as_str()), _ => None })
                    .collect::<Vec<_>>()
                    .join("\n");
                Some(serde_json::json!({"role": "tool", "tool_call_id": t.tool_call_id, "content": text}))
            }
            AgentMessage::CompactionSummary(c) => {
                Some(serde_json::json!({"role": "user", "content": format!("[Previous conversation summary]\n\n{}", c.summary)}))
            }
            AgentMessage::BranchSummary(b) => {
                Some(serde_json::json!({"role": "user", "content": format!("[Branch summary]\n\n{}", b.summary)}))
            }
            _ => None,
        })
        .collect()
}

// parse_response_events replaced by CollectedResponse::from_events

pub(crate) fn load_agents_md(cwd: &std::path::Path) -> Option<String> {
    let project = cwd.join("AGENTS.md");
    if project.exists() {
        return std::fs::read_to_string(&project).ok();
    }
    let global = config::global_dir().join("AGENTS.md");
    if global.exists() {
        return std::fs::read_to_string(&global).ok();
    }
    None
}
