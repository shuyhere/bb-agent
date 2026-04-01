use anyhow::Result;
use std::io::{self, Write};
use std::path::PathBuf;

use bb_core::agent::{self, DEFAULT_SYSTEM_PROMPT};
use bb_core::agent_loop::AgentLoopEvent;
use bb_core::config;
use bb_core::settings::Settings;
use bb_core::types::*;
use bb_hooks::EventBus;
use bb_provider::anthropic::AnthropicProvider;
use bb_provider::openai::OpenAiProvider;
use bb_provider::registry::{ApiType, Model, ModelRegistry};
use bb_provider::Provider;
use bb_session::{context, store};
use bb_tools::{builtin_tools, ToolContext};
use bb_tui::chat;
use bb_tui::editor::Editor;
use bb_tui::model_selector::ModelSelector;
use bb_tui::session_selector::SessionSelector;
use bb_tui::status;
use bb_tui::terminal::{ProcessTerminal, Terminal};
use crossterm::event::{self, Event};
use crossterm::style::{Color, Stylize};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::login;
use crate::session::AgentSession;
use crate::slash::{self, SlashResult};
use crate::Cli;

// ── Terminal cleanup guard ───────────────────────────────────────────

/// Ensures terminal state is restored on drop (including panics).
struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        crossterm::terminal::disable_raw_mode().ok();
        let mut stdout = io::stdout();
        crossterm::execute!(stdout, crossterm::cursor::Show).ok();
        stdout.flush().ok();
    }
}

// ── Interactive mode state ───────────────────────────────────────────

struct InteractiveMode {
    // Terminal
    terminal: ProcessTerminal,
    // Editor for user input
    editor: Editor,
    // Model info
    model: Model,
    registry: ModelRegistry,
    // Session
    session: AgentSession,
    cwd: PathBuf,
    // Tracking
    total_tokens: u64,
    // Running state
    cancel: Option<CancellationToken>,
    agent_running: bool,
}

// ── Public entry point ───────────────────────────────────────────────

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
    } else {
        store::create_session(&conn, cwd.to_str().unwrap_or("."))?
    };

    // Load settings
    let settings = Settings::load_merged(&cwd);

    // Parse model
    let model_input = cli.model.as_deref().or(settings.default_model.as_deref());
    let provider_input = cli
        .provider
        .as_deref()
        .or(settings.default_provider.as_deref());
    let (provider_name, model_id, _thinking_override) =
        crate::run::parse_model_arg(provider_input, model_input);

    // Load AGENTS.md
    let agents_md = crate::run::load_agents_md(&cwd);
    let base_prompt = cli
        .system_prompt
        .as_deref()
        .unwrap_or(DEFAULT_SYSTEM_PROMPT);
    let system_prompt = match &cli.append_system_prompt {
        Some(append) => agent::build_system_prompt(base_prompt, Some(append)),
        None => agent::build_system_prompt(base_prompt, agents_md.as_deref()),
    };

    // Model registry
    let mut registry = ModelRegistry::new();
    registry.load_custom_models(&settings);

    let model = registry
        .find(&provider_name, &model_id)
        .cloned()
        .or_else(|| {
            registry
                .find_fuzzy(&model_id, Some(&provider_name))
                .cloned()
        })
        .or_else(|| registry.find_fuzzy(&model_id, None).cloned())
        .unwrap_or_else(|| Model {
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

    // Resolve API key
    let api_key = match &cli.api_key {
        Some(k) => k.clone(),
        None => login::resolve_api_key(&provider_name).unwrap_or_default(),
    };

    if api_key.is_empty() {
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
    let tool_defs = crate::session::build_tool_defs(&tools);

    let _event_bus = EventBus::new();

    let provider: Box<dyn Provider> = match model.api {
        ApiType::AnthropicMessages => Box::new(AnthropicProvider::new()),
        _ => Box::new(OpenAiProvider::new()),
    };

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
        compaction_settings: settings.compaction_settings(),
    };

    let terminal = ProcessTerminal::new();

    let mut mode = InteractiveMode {
        terminal,
        editor: Editor::new("> "),
        model: model.clone(),
        registry,
        session,
        cwd,
        total_tokens: 0,
        cancel: None,
        agent_running: false,
    };

    // Print banner (before raw mode)
    println!("bb-agent v{}", env!("CARGO_PKG_VERSION"));
    println!("Type your prompt, or Ctrl+C to exit.");

    // Display status bar
    let status_line = status::render_status(
        Some(mode.model.name.as_str()),
        None,
        Some(mode.model.context_window),
    );
    if !status_line.is_empty() {
        println!("{status_line}");
    }
    println!();

    // If --continue, restore messages
    if cli.r#continue {
        if let Ok(ctx) = context::build_context(&mode.session.conn, &mode.session.session_id) {
            if !ctx.messages.is_empty() {
                restore_messages(&ctx.messages);
            }
        }
    }

    // If initial messages provided, run them first
    if !cli.messages.is_empty() {
        let prompt = cli.messages.join(" ");
        run_agent_turn(&mut mode, &prompt).await?;
    }

    // Main interactive loop using editor's read_line (handles raw mode internally)
    loop {
        let input = match mode.editor.read_line() {
            Some(input) => input,
            None => break, // Ctrl+C / Ctrl+D with empty buffer
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
                    .current_dir(&mode.cwd)
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
            if !handle_slash_command(&mut mode, &input) {
                break; // /exit or /quit
            }
            continue;
        }

        // Send to agent
        run_agent_turn(&mut mode, &input).await?;
    }

    println!("\nGoodbye!");
    Ok(())
}

// ── Slash command handler ────────────────────────────────────────────

/// Returns false if the loop should exit.
fn handle_slash_command(mode: &mut InteractiveMode, input: &str) -> bool {
    match slash::handle_slash_command(input) {
        SlashResult::Exit => return false,
        SlashResult::Handled => {}
        SlashResult::NewSession => {
            match store::create_session(
                &mode.session.conn,
                mode.cwd.to_str().unwrap_or("."),
            ) {
                Ok(new_id) => {
                    mode.session.session_id = new_id;
                    mode.total_tokens = 0;
                    println!("New session started.");
                }
                Err(e) => println!("Error creating session: {e}"),
            }
        }
        SlashResult::Compact(_instructions) => {
            println!("Compaction not yet implemented in interactive mode.");
        }
        SlashResult::ModelSelect(_search) => {
            if let Some(new_model) = run_model_selector(mode) {
                // Update provider if API type changed
                let needs_new_provider = !matches!(
                    (&new_model.api, &mode.model.api),
                    (ApiType::AnthropicMessages, ApiType::AnthropicMessages)
                        | (ApiType::OpenaiCompletions, ApiType::OpenaiCompletions)
                );
                if needs_new_provider {
                    mode.session.provider = match new_model.api {
                        ApiType::AnthropicMessages => Box::new(AnthropicProvider::new()),
                        _ => Box::new(OpenAiProvider::new()),
                    };
                }
                mode.session.api_key =
                    login::resolve_api_key(&new_model.provider).unwrap_or_default();
                mode.session.base_url = new_model
                    .base_url
                    .clone()
                    .unwrap_or_else(|| "https://api.openai.com/v1".into());
                println!("Switched to model: {}", new_model.name);
                mode.session.model = new_model.clone();
                mode.model = new_model;
            }
        }
        SlashResult::Resume => {
            if let Some(session_id) = run_session_selector(mode) {
                mode.session.session_id = session_id.clone();
                mode.total_tokens = 0;
                // Load and display existing messages
                if let Ok(ctx) =
                    context::build_context(&mode.session.conn, &mode.session.session_id)
                {
                    restore_messages(&ctx.messages);
                }
                println!(
                    "Resumed session {}.",
                    &session_id[..8.min(session_id.len())]
                );
            }
        }
        SlashResult::Tree => {
            println!("Tree navigation not yet implemented.");
        }
        SlashResult::Fork => {
            println!("Fork not yet implemented.");
        }
        SlashResult::Login => {
            println!("Run `bb login` from a separate terminal.");
        }
        SlashResult::Logout => {
            println!("Run `bb logout` from a separate terminal.");
        }
        SlashResult::SetName(name) => {
            println!("Session named: {name}");
        }
        SlashResult::NotCommand => {
            // Not a recognized command; already printed error in handle_slash_command
        }
    }
    true
}

// ── Model selector overlay ───────────────────────────────────────────

fn run_model_selector(mode: &mut InteractiveMode) -> Option<Model> {
    let mut selector = ModelSelector::new(&mode.registry, 15);
    let width = mode.terminal.columns();

    crossterm::terminal::enable_raw_mode().ok();

    let result = loop {
        let lines = selector.render(width);
        let mut stdout = io::stdout();
        write!(stdout, "\x1b[?2026h").ok();
        for line in &lines {
            write!(stdout, "\r{}\x1b[K\n", line).ok();
        }
        write!(stdout, "\x1b[?2026l").ok();
        stdout.flush().ok();

        if let Ok(Event::Key(key)) = event::read() {
            match selector.handle_key(key) {
                Some(Ok(selection)) => {
                    let model = mode
                        .registry
                        .find(&selection.provider, &selection.model_id)
                        .cloned();
                    break model;
                }
                Some(Err(())) => {
                    break None;
                }
                None => {
                    let mut stdout = io::stdout();
                    for _ in 0..lines.len() {
                        write!(stdout, "\x1b[A\x1b[K").ok();
                    }
                    stdout.flush().ok();
                }
            }
        }
    };

    crossterm::terminal::disable_raw_mode().ok();
    result
}

// ── Session selector overlay ─────────────────────────────────────────

fn run_session_selector(mode: &mut InteractiveMode) -> Option<String> {
    let sessions = store::list_sessions(
        &mode.session.conn,
        mode.cwd.to_str().unwrap_or("."),
    )
    .ok()?;
    if sessions.is_empty() {
        println!("No sessions to resume.");
        return None;
    }

    let mut selector = SessionSelector::new(sessions, 15);
    let width = mode.terminal.columns();

    crossterm::terminal::enable_raw_mode().ok();

    let result = loop {
        let lines = selector.render(width);
        let mut stdout = io::stdout();
        write!(stdout, "\x1b[?2026h").ok();
        for line in &lines {
            write!(stdout, "\r{}\x1b[K\n", line).ok();
        }
        write!(stdout, "\x1b[?2026l").ok();
        stdout.flush().ok();

        if let Ok(Event::Key(key)) = event::read() {
            match selector.handle_key(key) {
                Some(Ok(selection)) => break Some(selection.session_id),
                Some(Err(())) => break None,
                None => {
                    let mut stdout = io::stdout();
                    for _ in 0..lines.len() {
                        write!(stdout, "\x1b[A\x1b[K").ok();
                    }
                    stdout.flush().ok();
                }
            }
        }
    };

    crossterm::terminal::disable_raw_mode().ok();
    result
}

// ── Message display helpers ──────────────────────────────────────────

fn restore_messages(messages: &[AgentMessage]) {
    for msg in messages {
        let lines = chat::render_message(msg);
        for line in &lines {
            println!("{line}");
        }
    }
}

// ── Agent turn execution (uses AgentSession + event-driven display) ──

async fn run_agent_turn(mode: &mut InteractiveMode, prompt: &str) -> Result<()> {
    // Display user message
    println!("{}", "You".bold().with(Color::Blue));
    println!("  {prompt}");
    println!();

    // Create event channel
    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<AgentLoopEvent>();

    // Set up cancellation
    let cancel = CancellationToken::new();
    mode.cancel = Some(cancel.clone());
    mode.agent_running = true;

    // Spawn agent loop in background
    let prompt_owned = prompt.to_string();

    // We need to run the agent session's prompt. Since AgentSession holds
    // non-Send fields (rusqlite::Connection), we run it on the current task.
    // The streaming display happens as we drain events.
    let agent_result = {
        // Run the prompt (this drives the full agent loop internally)
        mode.session.run_prompt(&prompt_owned, event_tx).await
    };

    // Now drain all events that were buffered during the run
    // (In practice, since run_prompt is awaited above, events were sent
    // synchronously during streaming. We drain any remaining.)
    let mut started_text = false;
    let mut started_tool = false;
    let mut last_was_turn_start = false;

    // Process events that were already sent
    while let Ok(ev) = event_rx.try_recv() {
        display_agent_event(
            &ev,
            &mode.model,
            &mut started_text,
            &mut started_tool,
            &mut last_was_turn_start,
            &mut mode.total_tokens,
        );
    }

    // Ensure clean line ending
    if started_text || started_tool {
        println!();
    }
    if !last_was_turn_start {
        println!();
    }

    // Show status bar after turn
    let status_line = status::render_status(
        Some(mode.model.name.as_str()),
        if mode.total_tokens > 0 {
            Some(mode.total_tokens)
        } else {
            None
        },
        Some(mode.model.context_window),
    );
    if !status_line.is_empty() {
        println!("{status_line}");
    }

    mode.agent_running = false;
    mode.cancel = None;

    if let Err(e) = agent_result {
        eprintln!(
            "{}",
            format!("Agent error: {e}").with(Color::Red)
        );
    }

    Ok(())
}

/// Display a single agent loop event to the terminal.
fn display_agent_event(
    ev: &AgentLoopEvent,
    model: &Model,
    started_text: &mut bool,
    started_tool: &mut bool,
    last_was_turn_start: &mut bool,
    _total_tokens: &mut u64,
) {
    let mut stdout = io::stdout();

    match ev {
        AgentLoopEvent::TurnStart { .. } => {
            // Print assistant header
            print!(
                "{}{}",
                "Assistant".bold().with(Color::Green),
                format!(" ({})", model.id).with(Color::DarkGrey),
            );
            println!();
            *started_text = false;
            *started_tool = false;
            *last_was_turn_start = true;
        }
        AgentLoopEvent::TextDelta { text } => {
            if !*started_text {
                *started_text = true;
                print!("  ");
            }
            print!("{text}");
            stdout.flush().ok();
            *last_was_turn_start = false;
        }
        AgentLoopEvent::ThinkingDelta { .. } => {
            if !*started_text {
                *started_text = true;
                print!("  {}", "[thinking] ".with(Color::DarkGrey));
                stdout.flush().ok();
            }
            *last_was_turn_start = false;
        }
        AgentLoopEvent::ToolCallStart { name, .. } => {
            if *started_text {
                println!();
                *started_text = false;
            }
            print!(
                "  {} {}",
                "⚡".with(Color::Yellow),
                name.clone().bold(),
            );
            stdout.flush().ok();
            *started_tool = true;
            *last_was_turn_start = false;
        }
        AgentLoopEvent::ToolCallDelta { .. } => {}
        AgentLoopEvent::ToolExecuting { name, .. } => {
            if *started_tool {
                println!();
                *started_tool = false;
            }
            print!(
                "  {} {} ",
                "⏳",
                name.clone().with(Color::Cyan),
            );
            stdout.flush().ok();
            *last_was_turn_start = false;
        }
        AgentLoopEvent::ToolResult {
            name: _,
            content,
            is_error,
            ..
        } => {
            if *is_error {
                println!("{}", "✗".with(Color::Red));
                println!("    {}", content.clone().with(Color::Red));
            } else {
                println!("{}", "✓".with(Color::Green));
                // Show brief preview
                let preview_lines: Vec<&str> = content.lines().take(5).collect();
                for line in &preview_lines {
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
            println!();
            *last_was_turn_start = false;
        }
        AgentLoopEvent::TurnEnd { .. } => {
            if *started_text || *started_tool {
                println!();
                *started_text = false;
                *started_tool = false;
            }
        }
        AgentLoopEvent::AssistantDone => {
            *last_was_turn_start = false;
        }
        AgentLoopEvent::Error { message } => {
            if *started_text || *started_tool {
                println!();
            }
            eprintln!(
                "{}",
                format!("Error: {message}").with(Color::Red)
            );
            *started_text = false;
            *started_tool = false;
            *last_was_turn_start = false;
        }
    }
}
