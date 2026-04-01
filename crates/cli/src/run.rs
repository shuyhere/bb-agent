use anyhow::Result;

use bb_core::agent::{self, DEFAULT_SYSTEM_PROMPT};
use bb_core::agent_loop::AgentLoopEvent;
use bb_core::config;
use bb_core::types::*;
use bb_hooks::EventBus;
use bb_provider::anthropic::AnthropicProvider;
use bb_provider::openai::OpenAiProvider;
use bb_provider::registry::{ApiType, ModelRegistry};
use bb_provider::Provider;
use bb_session::store;
use bb_tools::{builtin_tools, ToolContext};
use bb_tui::app::App;

use crate::login;
use crate::session::{build_tool_defs, AgentSession};
use crate::slash::{self, SlashResult};
use crate::Cli;

pub async fn run_agent(cli: Cli) -> Result<()> {
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
        let sessions = store::list_sessions(&conn, cwd.to_str().unwrap_or("."))?;
        match sessions.first() {
            Some(s) => {
                tracing::info!("Continuing session {}", s.session_id);
                s.session_id.clone()
            }
            None => store::create_session(&conn, cwd.to_str().unwrap_or("."))?,
        }
    } else if cli.no_session {
        store::create_session(&conn, cwd.to_str().unwrap_or("."))?
    } else {
        store::create_session(&conn, cwd.to_str().unwrap_or("."))?
    };

    // Parse --model (supports "provider/model" and "model:thinking")
    let (provider_name, model_id, _thinking_override) =
        parse_model_arg(cli.provider.as_deref(), cli.model.as_deref());

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

    // Model registry
    let registry = ModelRegistry::new();
    let model = registry
        .find(&provider_name, &model_id)
        .cloned()
        .unwrap_or_else(|| bb_provider::registry::Model {
            id: model_id.clone(),
            name: model_id.clone(),
            provider: provider_name.clone(),
            api: bb_provider::registry::ApiType::OpenaiCompletions,
            context_window: 128_000,
            max_tokens: 16384,
            reasoning: false,
            base_url: None,
            cost: Default::default(),
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
    };
    let tool_defs = build_tool_defs(&tools);

    // Event bus (for future plugin support)
    let _event_bus = EventBus::new();

    // Provider — select based on API type
    let provider: Box<dyn Provider> = match model.api {
        ApiType::AnthropicMessages => Box::new(AnthropicProvider::new()),
        _ => Box::new(OpenAiProvider::new()),
    };

    // Build AgentSession
    let session = AgentSession {
        conn,
        session_id,
        system_prompt,
        model: model.clone(),
        provider,
        api_key,
        base_url,
        tools,
        tool_defs,
        tool_ctx,
        compaction_settings: CompactionSettings::default(),
    };

    // TUI app
    let mut app = App::new();
    app.set_model(&model.name);

    if cli.print && !cli.messages.is_empty() {
        // Print mode: single prompt
        let prompt = cli.messages.join(" ");
        run_prompt_with_display(&session, &prompt, &app).await?;
        return Ok(());
    }

    // Interactive mode
    app.print_banner();
    app.display_status(None, Some(model.context_window));

    // If initial messages provided, run them first
    if !cli.messages.is_empty() {
        let prompt = cli.messages.join(" ");
        run_prompt_with_display(&session, &prompt, &app).await?;
    }

    // Main interactive loop
    loop {
        let input = match app.read_input() {
            Some(input) => input,
            None => break,
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
                    let sessions =
                        store::list_sessions(&session.conn, cwd.to_str().unwrap_or("."))?;
                    if sessions.is_empty() {
                        println!("No sessions to resume.");
                    } else {
                        println!("Recent sessions:");
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
                    continue;
                }
                SlashResult::NotCommand => {} // fall through to LLM
            }
        }

        run_prompt_with_display(&session, &input, &app).await?;
    }

    println!("\nGoodbye!");
    Ok(())
}

/// Run a prompt through the AgentSession and display events inline.
async fn run_prompt_with_display(session: &AgentSession, prompt: &str, app: &App) -> Result<()> {
    use crossterm::style::{Color, Stylize};
    use std::io::Write;
    use tokio::sync::mpsc;

    // Display user message
    let user_msg = AgentMessage::User(UserMessage {
        content: vec![ContentBlock::Text {
            text: prompt.to_string(),
        }],
        timestamp: chrono::Utc::now().timestamp_millis(),
    });
    app.display_message(&user_msg);

    // Create event channel
    let (tx, mut rx) = mpsc::unbounded_channel();

    // Run the agent loop (sends events to tx)
    // We need to spawn this since run_prompt is async and we want to receive events
    // concurrently. However, since AgentSession uses &self (not Send-safe with Connection),
    // we process events after the loop completes by collecting them.
    //
    // For now, run synchronously: the agent loop sends events to tx,
    // then we drain rx after. In the future, with a proper async session,
    // we'd use spawn + streaming.

    let result = session.run_prompt(prompt, tx).await;

    // Print assistant header
    print!(
        "{}{} ",
        "Assistant".bold().with(Color::Green),
        format!(" ({})", session.model.id).with(Color::DarkGrey),
    );
    std::io::stdout().flush().ok();
    println!();

    // Drain and display events
    let mut started_text = false;
    while let Ok(event) = rx.try_recv() {
        match event {
            AgentLoopEvent::TextDelta { text } => {
                if !started_text {
                    started_text = true;
                    print!("  ");
                }
                print!("{text}");
                std::io::stdout().flush().ok();
            }
            AgentLoopEvent::ThinkingDelta { .. } => {
                if !started_text {
                    started_text = true;
                    print!("  {}", "[thinking] ".with(Color::DarkGrey));
                }
            }
            AgentLoopEvent::ToolCallStart { name, .. } => {
                if started_text {
                    println!();
                    started_text = false;
                }
                print!(
                    "  {} {} ",
                    "⚡".with(Color::Yellow),
                    name.bold(),
                );
                std::io::stdout().flush().ok();
            }
            AgentLoopEvent::ToolExecuting { name, .. } => {
                print!(
                    "\n  {} {} ",
                    "⏳".with(Color::Cyan),
                    name.with(Color::Cyan),
                );
                std::io::stdout().flush().ok();
            }
            AgentLoopEvent::ToolResult {
                content, is_error, ..
            } => {
                if is_error {
                    println!("{}", "✗".with(Color::Red));
                    println!("    {}", content.with(Color::Red));
                } else {
                    println!("{}", "✓".with(Color::Green));
                    let preview: Vec<&str> = content.lines().take(5).collect();
                    for line in &preview {
                        println!("    {}", line.with(Color::DarkGrey));
                    }
                    let total = content.lines().count();
                    if total > 5 {
                        println!(
                            "    {}",
                            format!("[{} more lines]", total - 5).with(Color::DarkGrey)
                        );
                    }
                }
            }
            AgentLoopEvent::TurnEnd { .. } => {
                if started_text {
                    println!();
                    started_text = false;
                }
                println!();
            }
            AgentLoopEvent::Error { message } => {
                println!();
                eprintln!("{}", format!("Error: {message}").with(Color::Red));
            }
            AgentLoopEvent::AssistantDone => {}
            AgentLoopEvent::TurnStart { .. } => {}
            AgentLoopEvent::ToolCallDelta { .. } => {}
        }
    }

    if started_text {
        println!();
        println!();
    }

    if let Err(e) = result {
        eprintln!("Agent error: {e}");
    }

    Ok(())
}

/// Parse --model flag. Supports:
///   "gpt-4o"                   -> (default_provider, "gpt-4o", None)
///   "openai/gpt-4o"            -> ("openai", "gpt-4o", None)
///   "sonnet:high"              -> (default, fuzzy "sonnet", Some("high"))
///   "anthropic/sonnet:high"    -> ("anthropic", fuzzy "sonnet", Some("high"))
fn parse_model_arg(
    provider: Option<&str>,
    model: Option<&str>,
) -> (String, String, Option<String>) {
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

fn load_agents_md(cwd: &std::path::Path) -> Option<String> {
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
