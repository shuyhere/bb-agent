use anyhow::Result;
use std::path::PathBuf;

use bb_core::agent::{self, DEFAULT_SYSTEM_PROMPT};
use bb_core::config;
use bb_core::types::*;
use bb_hooks::EventBus;
use bb_provider::anthropic::AnthropicProvider;
use bb_provider::openai::OpenAiProvider;
use bb_provider::registry::{ApiType, Model, ModelRegistry};
use bb_provider::streaming::CollectedResponse;
use bb_provider::{CompletionRequest, Provider, RequestOptions, StreamEvent};
use bb_session::{compaction, context, store, tree};
use bb_tools::{builtin_tools, Tool, ToolContext};
use bb_tui::editor::Editor;
use bb_tui::markdown::MarkdownRenderer;
use bb_tui::model_selector::ModelSelector;
use bb_tui::session_selector::SessionSelector;
use bb_tui::status;
use bb_tui::terminal::{ProcessTerminal, Terminal};
use chrono::Utc;
use crossterm::event::{self, Event};
use crossterm::style::{Color, Stylize};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::login;
use crate::slash::{self, SlashResult};
use crate::Cli;

// ── Rendered message types ───────────────────────────────────────────

enum RenderedMessage {
    User(Vec<String>),
    Assistant(Vec<String>),
    ToolResult(Vec<String>),
    Compaction(Vec<String>),
    Streaming(StreamingState),
}

struct StreamingState {
    text_buffer: String,
    thinking: bool,
    tool_lines: Vec<String>,
    markdown_renderer: MarkdownRenderer,
}

impl StreamingState {
    fn new() -> Self {
        Self {
            text_buffer: String::new(),
            thinking: false,
            tool_lines: Vec::new(),
            markdown_renderer: MarkdownRenderer::new(""),
        }
    }

    fn render(&mut self, width: u16) -> Vec<String> {
        let mut lines = Vec::new();

        // Header
        lines.push(format!(
            "{}",
            "Assistant".bold().with(Color::Green),
        ));

        // Thinking indicator
        if self.thinking && self.text_buffer.is_empty() {
            lines.push(format!(
                "  {}",
                "[thinking...]".with(Color::DarkGrey),
            ));
        }

        // Rendered markdown for text so far
        if !self.text_buffer.is_empty() {
            self.markdown_renderer.set_text(&self.text_buffer);
            let md_lines = self.markdown_renderer.render(width.saturating_sub(2));
            for l in md_lines {
                lines.push(format!("  {l}"));
            }
        }

        // Tool call lines
        for tl in &self.tool_lines {
            lines.push(tl.clone());
        }

        lines.push(String::new());
        lines
    }
}

impl RenderedMessage {
    fn lines(&self, _width: u16) -> Vec<String> {
        match self {
            RenderedMessage::User(l) => l.clone(),
            RenderedMessage::Assistant(l) => l.clone(),
            RenderedMessage::ToolResult(l) => l.clone(),
            RenderedMessage::Compaction(l) => l.clone(),
            RenderedMessage::Streaming(_) => {
                // Streaming should use render() with width; we handle this specially
                Vec::new()
            }
        }
    }
}

// ── Interactive mode state ───────────────────────────────────────────

#[allow(dead_code)]
struct InteractiveMode {
    // Terminal
    terminal: ProcessTerminal,
    // Chat history
    messages: Vec<RenderedMessage>,
    // Editor
    editor: Editor,
    // Model info
    model: Model,
    registry: ModelRegistry,
    api_key: String,
    base_url: String,
    // Session
    conn: rusqlite::Connection,
    session_id: String,
    cwd: PathBuf,
    system_prompt: String,
    // Tools
    tools: Vec<Box<dyn Tool>>,
    tool_defs: Vec<serde_json::Value>,
    tool_ctx: ToolContext,
    // Provider
    provider: Box<dyn Provider>,
    // Tracking
    total_tokens: u64,
    // Running state
    cancel: Option<CancellationToken>,
    agent_running: bool,
    // Status message (transient, e.g. errors)
    status_message: Option<String>,
    // Thinking level
    thinking: Option<String>,
}

impl InteractiveMode {
    #[allow(dead_code)]
    fn render_to_lines(&mut self) -> Vec<String> {
        let width = self.terminal.columns();
        let mut lines: Vec<String> = Vec::new();

        // 1. Chat messages
        for msg in &mut self.messages {
            match msg {
                RenderedMessage::Streaming(state) => {
                    lines.extend(state.render(width));
                }
                other => {
                    lines.extend(other.lines(width));
                }
            }
        }

        // 2. Status bar
        let status_line = status::render_status(
            Some(self.model.name.as_str()),
            if self.total_tokens > 0 { Some(self.total_tokens) } else { None },
            Some(self.model.context_window),
        );
        if !status_line.is_empty() {
            lines.push(status_line);
        }

        // 3. Transient status message
        if let Some(ref msg) = self.status_message {
            lines.push(format!("{}", msg.clone().with(Color::Yellow)));
        }

        // 4. Editor (only if not running agent)
        if !self.agent_running {
            lines.extend(self.editor.render(width));
        } else {
            lines.push(format!(
                "{}",
                "  [agent running... Ctrl+C to abort]".with(Color::DarkGrey),
            ));
        }

        lines
    }

    #[allow(dead_code)]
    fn render(&mut self) {
        let lines = self.render_to_lines();
        // We write directly to stdout in cooked mode for scrollback-based TUI
        // Use synchronized output to avoid flicker
        let mut stdout = std::io::stdout();
        use std::io::Write;
        write!(stdout, "\x1b[?2026h").ok(); // sync begin

        // Clear from cursor to end of screen, then print lines
        // For scrollback-based: just print the new lines
        for line in &lines {
            writeln!(stdout, "\r{}\x1b[K", line).ok();
        }

        write!(stdout, "\x1b[?2026l").ok(); // sync end
        stdout.flush().ok();
    }

    fn print_lines(&self, lines: &[String]) {
        let mut stdout = std::io::stdout();
        use std::io::Write;
        for line in lines {
            writeln!(stdout, "{}", line).ok();
        }
        stdout.flush().ok();
    }

    fn add_user_message(&mut self, text: &str) {
        let lines = vec![
            format!("{}", "You".bold().with(Color::Blue)),
            format!("  {text}"),
            String::new(),
        ];
        self.print_lines(&lines);
        self.messages.push(RenderedMessage::User(lines));
    }

    fn finalize_streaming(&mut self) {
        // Convert the last streaming message to a finalized assistant message
        let last = self.messages.last_mut();
        if let Some(RenderedMessage::Streaming(state)) = last {
            let width = self.terminal.columns();
            let final_lines = state.render(width);
            *last.unwrap() = RenderedMessage::Assistant(final_lines);
        }
    }

    fn start_streaming(&mut self) {
        self.messages.push(RenderedMessage::Streaming(StreamingState::new()));
        self.agent_running = true;
    }

    fn append_text_delta(&mut self, text: &str) {
        if let Some(RenderedMessage::Streaming(state)) = self.messages.last_mut() {
            state.text_buffer.push_str(text);
            state.thinking = false;
        }
    }

    fn set_thinking(&mut self) {
        if let Some(RenderedMessage::Streaming(state)) = self.messages.last_mut() {
            state.thinking = true;
        }
    }

    fn add_tool_call_line(&mut self, name: &str) {
        let line = format!(
            "  {} {}",
            "⚡".with(Color::Yellow),
            name.bold(),
        );
        if let Some(RenderedMessage::Streaming(state)) = self.messages.last_mut() {
            state.tool_lines.push(line);
        }
    }

    fn add_tool_result_display(&mut self, name: &str, content: &[ContentBlock], is_error: bool) {
        let status = if is_error {
            "✗".with(Color::Red).to_string()
        } else {
            "✓".with(Color::Green).to_string()
        };

        let mut lines = vec![format!(
            "  {} {} result:",
            status,
            name.with(Color::Cyan),
        )];

        for block in content {
            if let ContentBlock::Text { text } = block {
                let preview_lines: Vec<&str> = text.lines().take(5).collect();
                for l in &preview_lines {
                    lines.push(format!("    {}", l.with(Color::DarkGrey)));
                }
                let total = text.lines().count();
                if total > 5 {
                    lines.push(format!(
                        "    {}",
                        format!("[{} more lines]", total - 5).with(Color::DarkGrey),
                    ));
                }
            }
        }
        lines.push(String::new());

        self.print_lines(&lines);
        self.messages.push(RenderedMessage::ToolResult(lines));
    }

    /// Run the model selector overlay
    fn run_model_selector(&mut self) -> Option<Model> {
        let mut selector = ModelSelector::new(&self.registry, 15);
        let width = self.terminal.columns();

        // Enter raw mode for selector
        crossterm::terminal::enable_raw_mode().ok();

        let result = loop {
            // Render selector
            let lines = selector.render(width);
            let mut stdout = std::io::stdout();
            use std::io::Write;
            // Clear area and draw
            write!(stdout, "\x1b[?2026h").ok();
            for line in &lines {
                write!(stdout, "\r{}\x1b[K\n", line).ok();
            }
            write!(stdout, "\x1b[?2026l").ok();
            stdout.flush().ok();

            // Wait for key
            if let Ok(Event::Key(key)) = event::read() {
                match selector.handle_key(key) {
                    Some(Ok(selection)) => {
                        // Find the full model
                        let model = self.registry.find(&selection.provider, &selection.model_id).cloned();
                        break model;
                    }
                    Some(Err(())) => {
                        break None;
                    }
                    None => {
                        // Clear previous selector lines and continue
                        let mut stdout = std::io::stdout();
                        // Move up and clear
                        for _ in 0..lines.len() {
                            write!(stdout, "\x1b[A\x1b[K").ok();
                        }
                        stdout.flush().ok();
                    }
                }
            }
        };

        crossterm::terminal::disable_raw_mode().ok();

        // Clear selector output
        let mut stdout = std::io::stdout();
        use std::io::Write;
        stdout.flush().ok();

        result
    }

    /// Run the session selector overlay
    fn run_session_selector(&mut self) -> Option<String> {
        let sessions = store::list_sessions(&self.conn, self.cwd.to_str().unwrap_or(".")).ok()?;
        if sessions.is_empty() {
            println!("No sessions to resume.");
            return None;
        }

        let mut selector = SessionSelector::new(sessions, 15);
        let width = self.terminal.columns();

        crossterm::terminal::enable_raw_mode().ok();

        let result = loop {
            let lines = selector.render(width);
            let mut stdout = std::io::stdout();
            use std::io::Write;
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
                        let mut stdout = std::io::stdout();
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

    async fn run_compaction(&mut self, custom_instructions: Option<&str>) -> Result<bool> {
        let compaction_settings = CompactionSettings::default();
        let path = tree::active_path(&self.conn, &self.session_id)?;
        let prep = match compaction::prepare_compaction(&path, &compaction_settings) {
            Some(p) => p,
            None => return Ok(false),
        };

        let cancel = CancellationToken::new();
        let result = compaction::compact(
            &prep,
            self.provider.as_ref(),
            &self.model.id,
            &self.api_key,
            &self.base_url,
            custom_instructions,
            cancel,
        ).await?;

        let comp_entry = SessionEntry::Compaction {
            base: EntryBase {
                id: EntryId::generate(),
                parent_id: get_leaf(&self.conn, &self.session_id),
                timestamp: Utc::now(),
            },
            summary: result.summary,
            first_kept_entry_id: EntryId(result.first_kept_entry_id),
            tokens_before: result.tokens_before,
            details: Some(serde_json::json!({
                "readFiles": result.read_files,
                "modifiedFiles": result.modified_files,
            })),
            from_plugin: false,
        };
        store::append_entry(&self.conn, &self.session_id, &comp_entry)?;

        println!("📦 Context compacted ({} tokens summarized)", result.tokens_before);
        Ok(true)
    }

    async fn handle_slash_command(&mut self, input: &str) -> bool {
        match slash::handle_slash_command(input) {
            SlashResult::Exit => return false,
            SlashResult::Handled => {}
            SlashResult::NewSession => {
                match store::create_session(&self.conn, self.cwd.to_str().unwrap_or(".")) {
                    Ok(new_id) => {
                        self.session_id = new_id;
                        self.messages.clear();
                        self.total_tokens = 0;
                        println!("New session started.");
                    }
                    Err(e) => println!("Error creating session: {e}"),
                }
            }
            SlashResult::Compact(instructions) => {
                match self.run_compaction(instructions.as_deref()).await {
                    Ok(true) => {},
                    Ok(false) => println!("Nothing to compact."),
                    Err(e) => println!("Compaction error: {e}"),
                }
            }
            SlashResult::ModelSelect(_search) => {
                if let Some(new_model) = self.run_model_selector() {
                    // Update provider if API type changed
                    if !matches!((&new_model.api, &self.model.api), (ApiType::AnthropicMessages, ApiType::AnthropicMessages) | (ApiType::OpenaiCompletions, ApiType::OpenaiCompletions)) {
                        self.provider = match new_model.api {
                            ApiType::AnthropicMessages => Box::new(AnthropicProvider::new()),
                            _ => Box::new(OpenAiProvider::new()),
                        };
                    }
                    // Re-resolve API key for new provider
                    self.api_key = login::resolve_api_key(&new_model.provider).unwrap_or_default();
                    self.base_url = new_model.base_url.clone()
                        .unwrap_or_else(|| "https://api.openai.com/v1".into());
                    // Persist model change to session
                    let entry = SessionEntry::ModelChange {
                        base: EntryBase {
                            id: EntryId::generate(),
                            parent_id: get_leaf(&self.conn, &self.session_id),
                            timestamp: Utc::now(),
                        },
                        provider: new_model.provider.clone(),
                        model_id: new_model.id.clone(),
                    };
                    if let Err(e) = store::append_entry(&self.conn, &self.session_id, &entry) {
                        eprintln!("Warning: failed to persist model change: {e}");
                    }
                    println!("Switched to model: {}", new_model.name);
                    self.model = new_model;
                }
            }
            SlashResult::Resume => {
                if let Some(session_id) = self.run_session_selector() {
                    self.session_id = session_id;
                    self.messages.clear();
                    self.total_tokens = 0;
                    // Load and display existing messages
                    if let Ok(ctx) = context::build_context(&self.conn, &self.session_id) {
                        self.restore_messages(&ctx.messages);
                    }
                    println!("Resumed session {}.", &self.session_id[..8.min(self.session_id.len())]);
                }
            }
            SlashResult::Tree => {
                println!("Tree navigation not yet implemented.");
            }
            SlashResult::Fork => {
                println!("Fork not yet implemented.");
            }
            SlashResult::Login => {
                // Can't await in sync context easily; print instructions
                println!("Run `bb login` from a separate terminal.");
            }
            SlashResult::Logout => {
                println!("Run `bb logout` from a separate terminal.");
            }
            SlashResult::SetName(name) => {
                println!("Session named: {name}");
            }
            SlashResult::NotCommand => {
                // Not a slash command, treat as regular input
                return true; // signal: send to LLM
            }
        }
        true // continue loop
    }

    fn restore_messages(&mut self, messages: &[AgentMessage]) {
        let width = self.terminal.columns();
        for msg in messages {
            let lines = bb_tui::chat::render_message(msg);
            let rendered = match msg {
                AgentMessage::User(_) => RenderedMessage::User(lines),
                AgentMessage::Assistant(_) => RenderedMessage::Assistant(lines),
                AgentMessage::ToolResult(_) => RenderedMessage::ToolResult(lines),
                AgentMessage::CompactionSummary(_) | AgentMessage::BranchSummary(_) => {
                    RenderedMessage::Compaction(lines)
                }
                _ => RenderedMessage::User(lines), // fallback
            };
            self.print_lines(&rendered.lines(width));
            self.messages.push(rendered);
        }
    }
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
    let cwd_str = cwd.to_str().unwrap_or(".");
    let session_id = if let Some(session_arg) = &cli.session {
        // --session: resolve by prefix
        let all_sessions = store::list_sessions(&conn, cwd_str)?;
        let matches: Vec<_> = all_sessions.iter()
            .filter(|s| s.session_id.starts_with(session_arg.as_str()))
            .collect();
        match matches.len() {
            1 => matches[0].session_id.clone(),
            0 => anyhow::bail!("No session matching '{}'", session_arg),
            n => anyhow::bail!("{n} sessions match '{}', be more specific", session_arg),
        }
    } else if cli.r#continue {
        let sessions = store::list_sessions(&conn, cwd_str)?;
        match sessions.first() {
            Some(s) => {
                tracing::info!("Continuing session {}", s.session_id);
                s.session_id.clone()
            }
            None => store::create_session(&conn, cwd_str)?,
        }
    } else if cli.no_session {
        store::create_session(&conn, cwd_str)?
    } else {
        store::create_session(&conn, cwd_str)?
    };

    // Parse model
    let (provider_name, model_id, _thinking_override) = crate::run::parse_model_arg(
        cli.provider.as_deref(),
        cli.model.as_deref(),
    );

    // Load AGENTS.md
    let agents_md = crate::run::load_agents_md(&cwd);
    let base_prompt = cli.system_prompt.as_deref().unwrap_or(DEFAULT_SYSTEM_PROMPT);
    let system_prompt = match &cli.append_system_prompt {
        Some(append) => agent::build_system_prompt(base_prompt, Some(append)),
        None => agent::build_system_prompt(base_prompt, agents_md.as_deref()),
    };

    // Model registry
    let registry = ModelRegistry::new();
    let model = registry
        .find(&provider_name, &model_id)
        .cloned()
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

    let base_url = model.base_url.clone()
        .unwrap_or_else(|| "https://api.openai.com/v1".into());

    // Tools
    // Tools — apply --tools / --no-tools filtering
    let tools: Vec<Box<dyn Tool>> = if cli.no_tools {
        vec![]
    } else if let Some(tools_str) = &cli.tools {
        let tool_names: Vec<&str> = tools_str.split(',').map(|s| s.trim()).collect();
        builtin_tools()
            .into_iter()
            .filter(|t| tool_names.contains(&t.name()))
            .collect()
    } else {
        builtin_tools()
    };
    let tool_ctx = ToolContext {
        cwd: cwd.clone(),
        artifacts_dir: artifacts_dir.clone(),
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

    let _event_bus = EventBus::new();

    let provider: Box<dyn Provider> = match model.api {
        ApiType::AnthropicMessages => Box::new(AnthropicProvider::new()),
        _ => Box::new(OpenAiProvider::new()),
    };

    let terminal = ProcessTerminal::new();

    let mut mode = InteractiveMode {
        terminal,
        messages: Vec::new(),
        editor: Editor::new("> "),
        model: model.clone(),
        registry,
        api_key,
        base_url,
        conn,
        session_id,
        cwd,
        system_prompt,
        tools,
        tool_defs,
        tool_ctx,
        provider,
        total_tokens: 0,
        cancel: None,
        agent_running: false,
        status_message: None,
        thinking: cli.thinking.clone(),
    };

    // Print banner
    println!("bb-agent v{}", env!("CARGO_PKG_VERSION"));
    println!("Type your prompt, or Ctrl+C to exit.");

    // Display status
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
        if let Ok(ctx) = context::build_context(&mode.conn, &mode.session_id) {
            if !ctx.messages.is_empty() {
                mode.restore_messages(&ctx.messages);
            }
        }
    }

    // If initial messages provided, run them first
    if !cli.messages.is_empty() {
        let prompt = cli.messages.join(" ");
        run_agent_turn(&mut mode, &prompt).await?;
    }

    // Main interactive loop
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
            let result = slash::handle_slash_command(&input);
            match result {
                SlashResult::Exit => break,
                SlashResult::NotCommand => {
                    // Send to LLM
                }
                _ => {
                    // Handle in method (for model/session selectors etc.)
                    mode.handle_slash_command(&input).await;
                    continue;
                }
            }
        }

        // Send to agent
        run_agent_turn(&mut mode, &input).await?;
    }

    println!("\nGoodbye!");
    Ok(())
}

// ── Agent turn execution ─────────────────────────────────────────────

async fn run_agent_turn(mode: &mut InteractiveMode, prompt: &str) -> Result<()> {
    use std::io::Write;

    // Append user message to session
    let user_entry = SessionEntry::Message {
        base: EntryBase {
            id: EntryId::generate(),
            parent_id: get_leaf(&mode.conn, &mode.session_id),
            timestamp: Utc::now(),
        },
        message: AgentMessage::User(UserMessage {
            content: vec![ContentBlock::Text { text: prompt.to_string() }],
            timestamp: Utc::now().timestamp_millis(),
        }),
    };
    store::append_entry(&mode.conn, &mode.session_id, &user_entry)?;
    mode.add_user_message(prompt);

    // Agent loop (tool use can cause multiple turns)
    loop {
        let ctx = context::build_context(&mode.conn, &mode.session_id)?;
        let provider_messages = crate::run::messages_to_provider(&ctx.messages);

        let request = CompletionRequest {
            system_prompt: mode.system_prompt.clone(),
            messages: provider_messages,
            tools: mode.tool_defs.clone(),
            model: mode.model.id.clone(),
            max_tokens: Some(mode.model.max_tokens as u32),
            stream: true,
            thinking: mode.thinking.clone(),
        };

        let cancel = CancellationToken::new();
        mode.cancel = Some(cancel.clone());

        let options = RequestOptions {
            api_key: mode.api_key.clone(),
            base_url: mode.base_url.clone(),
            headers: std::collections::HashMap::new(),
            cancel: cancel.clone(),
        };

        // Start streaming display
        mode.start_streaming();

        // Print assistant header
        print!(
            "{}{} ",
            "Assistant".bold().with(Color::Green),
            format!(" ({})", mode.model.id).with(Color::DarkGrey),
        );
        std::io::stdout().flush().ok();

        let (tx, mut rx) = mpsc::unbounded_channel();

        // Spawn abort listener: cancel on Escape or Ctrl+C during streaming
        let abort_cancel = cancel.clone();
        let abort_handle = tokio::task::spawn_blocking(move || {
            crossterm::terminal::enable_raw_mode().ok();
            loop {
                if abort_cancel.is_cancelled() {
                    break;
                }
                if crossterm::event::poll(std::time::Duration::from_millis(50)).unwrap_or(false) {
                    if let Ok(Event::Key(key)) = crossterm::event::read() {
                        use crossterm::event::KeyCode;
                        match key.code {
                            KeyCode::Esc => {
                                abort_cancel.cancel();
                                break;
                            }
                            KeyCode::Char('c') if key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL) => {
                                abort_cancel.cancel();
                                break;
                            }
                            _ => {}
                        }
                    }
                }
            }
            crossterm::terminal::disable_raw_mode().ok();
        });

        // Spawn the streaming request
        let stream_result = mode.provider.stream(request, options, tx).await;
        if let Err(e) = stream_result {
            println!();
            eprintln!("{}", format!("Provider error: {e}").with(Color::Red));
            mode.agent_running = false;
            mode.cancel = None;
            // Remove the streaming message
            if matches!(mode.messages.last(), Some(RenderedMessage::Streaming(_))) {
                mode.messages.pop();
            }
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
                    print!("{text}");
                    std::io::stdout().flush().ok();
                    mode.append_text_delta(text);
                }
                StreamEvent::ThinkingDelta { text: _ } => {
                    if !started_text {
                        started_text = true;
                        print!("  {}", "[thinking] ".with(Color::DarkGrey));
                        std::io::stdout().flush().ok();
                    }
                    mode.set_thinking();
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
                    mode.add_tool_call_line(name);
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
                StreamEvent::Usage(usage) => {
                    mode.total_tokens = usage.input_tokens + usage.output_tokens;
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

        // Stop the abort listener
        cancel.cancel(); // signal listener to stop if still running
        let _ = abort_handle.await; // wait for it to finish

        // Check if aborted
        let was_aborted = cancel.is_cancelled() && all_events.iter().any(|e| matches!(e, StreamEvent::Done));

        // Finalize streaming message
        mode.finalize_streaming();
        mode.agent_running = false;
        mode.cancel = None;

        if was_aborted && all_events.iter().all(|e| !matches!(e, StreamEvent::TextDelta { .. })) {
            println!("  {}", "[Aborted]".with(Color::Yellow));
            break;
        }

        // Collect final response
        let collected = CollectedResponse::from_events(&all_events);

        // Update token count
        if collected.input_tokens > 0 || collected.output_tokens > 0 {
            mode.total_tokens = collected.input_tokens + collected.output_tokens;
        }

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
            provider: mode.model.provider.clone(),
            model: mode.model.id.clone(),
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
                parent_id: get_leaf(&mode.conn, &mode.session_id),
                timestamp: Utc::now(),
            },
            message: assistant_msg,
        };
        store::append_entry(&mode.conn, &mode.session_id, &asst_entry)?;

        if collected.tool_calls.is_empty() {
            // Print status bar after turn
            let status_line = status::render_status(
                Some(mode.model.name.as_str()),
                if mode.total_tokens > 0 { Some(mode.total_tokens) } else { None },
                Some(mode.model.context_window),
            );
            if !status_line.is_empty() {
                println!("{status_line}");
            }
            break;
        }

        // Execute tool calls
        let tool_cancel = CancellationToken::new();
        for tc in &collected.tool_calls {
            let args: serde_json::Value =
                serde_json::from_str(&tc.arguments).unwrap_or(serde_json::json!({}));

            print!(
                "  {} {} ",
                "⏳",
                tc.name.clone().with(Color::Cyan),
            );
            std::io::stdout().flush().ok();

            let tool = mode.tools.iter().find(|t| t.name() == tc.name);
            let result = match tool {
                Some(t) => t.execute(args, &mode.tool_ctx, tool_cancel.clone()).await,
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

            // Store tool result
            let tool_result_msg = AgentMessage::ToolResult(ToolResultMessage {
                tool_call_id: tc.id.clone(),
                tool_name: tc.name.clone(),
                content: content.clone(),
                details: None,
                is_error,
                timestamp: Utc::now().timestamp_millis(),
            });

            let tr_entry = SessionEntry::Message {
                base: EntryBase {
                    id: EntryId::generate(),
                    parent_id: get_leaf(&mode.conn, &mode.session_id),
                    timestamp: Utc::now(),
                },
                message: tool_result_msg,
            };
            store::append_entry(&mode.conn, &mode.session_id, &tr_entry)?;

            // Track in rendered messages
            mode.add_tool_result_display(&tc.name, &content, is_error);
        }

        println!();

        // Auto-compaction check after tool-use turn
        let compaction_settings = CompactionSettings::default();
        let ctx_check = context::build_context(&mode.conn, &mode.session_id)?;
        let total_tokens: u64 = ctx_check.messages.iter()
            .map(|m| compaction::estimate_tokens_text(&serde_json::to_string(m).unwrap_or_default()))
            .sum();

        if compaction::should_compact(total_tokens, mode.model.context_window, &compaction_settings) {
            let path = tree::active_path(&mode.conn, &mode.session_id)?;
            if let Some(prep) = compaction::prepare_compaction(&path, &compaction_settings) {
                let cancel_compact = CancellationToken::new();
                match compaction::compact(
                    &prep, mode.provider.as_ref(), &mode.model.id,
                    &mode.api_key, &mode.base_url, None, cancel_compact,
                ).await {
                    Ok(result) => {
                        let comp_entry = SessionEntry::Compaction {
                            base: EntryBase {
                                id: EntryId::generate(),
                                parent_id: get_leaf(&mode.conn, &mode.session_id),
                                timestamp: Utc::now(),
                            },
                            summary: result.summary,
                            first_kept_entry_id: EntryId(result.first_kept_entry_id),
                            tokens_before: result.tokens_before,
                            details: Some(serde_json::json!({
                                "readFiles": result.read_files,
                                "modifiedFiles": result.modified_files,
                            })),
                            from_plugin: false,
                        };
                        store::append_entry(&mode.conn, &mode.session_id, &comp_entry)?;
                        println!("📦 Context compacted ({} tokens summarized)", result.tokens_before);
                    }
                    Err(e) => {
                        eprintln!("Auto-compaction failed: {e}");
                    }
                }
            }
        }

        // Continue the agent loop for the next turn
    }

    Ok(())
}

fn get_leaf(conn: &rusqlite::Connection, session_id: &str) -> Option<EntryId> {
    store::get_session(conn, session_id)
        .ok()
        .flatten()
        .and_then(|s| s.leaf_id.map(EntryId))
}
