//! Interactive mode — scrollback-based TUI matching pi's visual style.
//!
//! Architecture (matching pi):
//! - Component tree: header → chat → editor → footer
//! - Async event loop polling keyboard events + agent response events
//! - Differential rendering with synchronized output
//! - Bordered editor box (not > prompt)
//! - Real footer with cost/tokens/model

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
use bb_tui::component::{Component, Container, Focusable, Spacer, Text};
use bb_tui::editor::Editor;
use bb_tui::footer::{Footer, FooterData};
use bb_tui::markdown::MarkdownRenderer;
use bb_tui::terminal::TerminalEvent;
use bb_tui::tui_core::TUI;
use chrono::Utc;
use crossterm::event::{KeyCode, KeyModifiers};
use crossterm::style::{Color, Stylize};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::login;
use crate::slash::{self, SlashResult};
use crate::Cli;

// ── Styling helpers ─────────────────────────────────────────────────

fn dim(text: &str) -> String {
    format!("\x1b[2m{}\x1b[0m", text)
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

fn style_tool_ok() -> String {
    format!("{}", "✓".with(Color::Green))
}

fn style_tool_err() -> String {
    format!("{}", "✗".with(Color::Red))
}

fn style_error(text: &str) -> String {
    format!("{}", text.with(Color::Red))
}

// ── Agent events (sent from the agent task to the render loop) ──────

#[derive(Debug)]
enum AgentEvent {
    /// Streaming text delta from assistant.
    TextDelta(String),
    /// Thinking indicator.
    ThinkingDelta(String),
    /// Tool call started.
    ToolStart { name: String, id: String },
    /// Tool execution result.
    ToolResult {
        name: String,
        success: bool,
        preview: String,
    },
    /// Assistant turn complete (text + tool calls collected).
    TurnComplete {
        text: String,
        has_tool_calls: bool,
        input_tokens: u64,
        output_tokens: u64,
    },
    /// Need to continue (tool calls need execution then another turn).
    NeedsContinue,
    /// Error.
    Error(String),
    /// Done — no more turns needed.
    Done,
}

// ── Banner component ────────────────────────────────────────────────

fn make_header(model_name: &str) -> Text {
    let lines = vec![
        String::new(),
        format!(" {} v{}", "bb-agent".with(Color::Cyan).bold(), env!("CARGO_PKG_VERSION")),
        format!(" {}  {}", dim("enter"), "to submit"),
        format!(" {}  {}", dim("alt+enter"), "for newline"),
        format!(" {}  {}", dim("ctrl+c"), "to cancel/clear"),
        format!(" {}  {}", dim("ctrl+d"), "to exit"),
        format!(" {}  {}", dim("/"), "for commands"),
        String::new(),
    ];
    Text { lines }
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
    let cwd_str = cwd.to_str().unwrap_or(".").to_string();

    let session_id = if cli.r#continue {
        let sessions = store::list_sessions(&conn, &cwd_str)?;
        match sessions.first() {
            Some(s) => s.session_id.clone(),
            None => store::create_session(&conn, &cwd_str)?,
        }
    } else {
        store::create_session(&conn, &cwd_str)?
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

    // ── Build the TUI component tree ──
    // Layout: header → chat → spacer → editor → footer

    let mut tui = TUI::new();

    // Index 0: Header
    let header = make_header(&model.name);
    tui.root.add(Box::new(header));

    // Index 1: Chat container (messages accumulate here)
    let chat_container = Container::new();
    tui.root.add(Box::new(chat_container));

    // Index 2: Spacer before editor
    tui.root.add(Box::new(Spacer::new(0)));

    // Index 3: Editor (focused)
    let mut editor = Editor::new();
    editor.terminal_rows = tui.rows();
    <Editor as Focusable>::set_focused(&mut editor, true);
    tui.root.add(Box::new(editor));

    // Index 4: Footer
    let footer = Footer::new(FooterData {
        model_name: model.name.clone(),
        provider: model.provider.clone(),
        cwd: cwd_str.clone(),
        context_window: model.context_window,
        ..Default::default()
    });
    tui.root.add(Box::new(footer));

    // Focus on the editor (index 3)
    tui.set_focus(Some(3));

    // Start the TUI and get the event channel
    let mut term_events = tui.start();

    // Initial render
    tui.render();

    // If --continue, populate chat with restored messages
    if cli.r#continue {
        if let Ok(ctx) = context::build_context(&conn, &session_id) {
            for msg in &ctx.messages {
                let lines = bb_tui::chat::render_message(msg);
                add_lines_to_chat(&mut tui, lines);
            }
            tui.render();
        }
    }

    // Channel for agent events
    let (agent_tx, mut agent_rx) = mpsc::unbounded_channel::<AgentEvent>();

    // Track cumulative stats
    let mut total_input_tokens: u64 = 0;
    let mut total_output_tokens: u64 = 0;
    let mut total_cost: f64 = 0.0;
    let mut running = true;
    let mut agent_running = false;
    let mut cancel_token = CancellationToken::new();
    let mut streaming = StreamingState::new();

    // If initial prompt, send it
    if !cli.messages.is_empty() {
        let prompt = cli.messages.join(" ");
        append_user_message(&conn, &session_id, &prompt)?;
        add_user_to_chat(&mut tui, &prompt);
        tui.render();

        agent_running = true;
        spawn_agent_turn(
            conn_path(&global_dir),
            session_id.clone(),
            system_prompt.clone(),
            model.clone(),
            provider_name.clone(),
            api_key.clone(),
            base_url.clone(),
            tool_defs.clone(),
            cli.thinking.clone(),
            agent_tx.clone(),
            cancel_token.clone(),
        );
    }

    // ── Main event loop ──
    loop {
        tokio::select! {
            // Terminal input events
            Some(event) = term_events.recv() => {
                match event {
                    TerminalEvent::Key(key) => {
                        // Match specific key combinations

                        // Ctrl+D — exit if editor empty
                        if key.code == KeyCode::Char('d') && key.modifiers == KeyModifiers::CONTROL {
                            // Get editor text
                            let editor = get_editor_mut(&mut tui);
                            if editor.get_text().trim().is_empty() {
                                running = false;
                            }
                        }

                        // Ctrl+C — cancel agent or clear editor
                        if key.code == KeyCode::Char('c') && key.modifiers == KeyModifiers::CONTROL {
                            if agent_running {
                                cancel_token.cancel();
                                cancel_token = CancellationToken::new();
                                agent_running = false;
                                add_status_to_chat(&mut tui, &dim("[cancelled]"));
                            } else {
                                let editor = get_editor_mut(&mut tui);
                                editor.clear();
                            }
                            tui.render();
                            continue;
                        }

                        // Enter — submit if not during agent turn
                        if key.code == KeyCode::Enter && key.modifiers == KeyModifiers::NONE && !agent_running {
                            let editor = get_editor_mut(&mut tui);
                            if let Some(text) = editor.try_submit() {
                                let text = text.trim().to_string();
                                if text.is_empty() {
                                    tui.render();
                                    continue;
                                }

                                editor.add_to_history(&text);

                                // Bash shortcut
                                if text.starts_with('!') {
                                    handle_bash(&text, &cwd, &mut tui);
                                    tui.render();
                                    continue;
                                }

                                // Slash commands
                                if text.starts_with('/') {
                                    match handle_slash(&text, &conn, &session_id, &cwd_str).await {
                                        Ok(true) => {
                                            running = false;
                                            break;
                                        }
                                        Ok(false) => {
                                            tui.render();
                                            continue;
                                        }
                                        Err(e) => {
                                            add_status_to_chat(&mut tui, &style_error(&format!("Error: {e}")));
                                            tui.render();
                                            continue;
                                        }
                                    }
                                }

                                // Send to agent
                                append_user_message(&conn, &session_id, &text)?;
                                add_user_to_chat(&mut tui, &text);
                                tui.render();

                                agent_running = true;
                                cancel_token = CancellationToken::new();
                                spawn_agent_turn(
                                    conn_path(&global_dir),
                                    session_id.clone(),
                                    system_prompt.clone(),
                                    model.clone(),
                                    provider_name.clone(),
                                    api_key.clone(),
                                    base_url.clone(),
                                    tool_defs.clone(),
                                    cli.thinking.clone(),
                                    agent_tx.clone(),
                                    cancel_token.clone(),
                                );
                            }
                            tui.render();
                            continue;
                        }

                        // Forward all other keys to focused component (editor)
                        tui.handle_key(&key);
                        // Update editor terminal_rows
                        let rows = tui.rows();
                        let editor = get_editor_mut(&mut tui);
                        editor.terminal_rows = rows;
                        tui.render();
                    }

                    TerminalEvent::Paste(text) => {
                        tui.handle_raw_input(&text);
                        tui.render();
                    }

                    TerminalEvent::Resize(_, _) => {
                        let rows = tui.rows();
                        let editor = get_editor_mut(&mut tui);
                        editor.terminal_rows = rows;
                        tui.force_render();
                    }

                    TerminalEvent::Raw(_) => {}
                }
            }

            // Agent events
            Some(agent_event) = agent_rx.recv() => {
                match agent_event {
                    AgentEvent::TextDelta(text) => {
                        streaming.append(&mut tui, &text);
                        tui.render();
                    }

                    AgentEvent::ThinkingDelta(_) => {
                        // Could show thinking indicator
                    }

                    AgentEvent::ToolStart { name, .. } => {
                        add_status_to_chat(&mut tui, &style_tool_call(&name));
                        tui.render();
                    }

                    AgentEvent::ToolResult { name, success, preview } => {
                        let status = if success { style_tool_ok() } else { style_tool_err() };
                        let lines = vec![
                            format!("  {} {} result:", status, name.with(Color::Cyan)),
                            format!("    {}", dim(&preview)),
                        ];
                        add_lines_to_chat(&mut tui, lines);
                        tui.render();
                    }

                    AgentEvent::TurnComplete { text, input_tokens, output_tokens, .. } => {
                        total_input_tokens += input_tokens;
                        total_output_tokens += output_tokens;

                        // Finalize streamed text with markdown rendering
                        streaming.finalize(&mut tui, &text);

                        // Update footer
                        update_footer(&mut tui, &model, &cwd_str, total_input_tokens, total_output_tokens, total_cost);
                        tui.render();
                    }

                    AgentEvent::NeedsContinue => {
                        // The agent task will spawn another turn
                    }

                    AgentEvent::Error(msg) => {
                        add_status_to_chat(&mut tui, &style_error(&format!("Error: {msg}")));
                        agent_running = false;
                        tui.render();
                    }

                    AgentEvent::Done => {
                        agent_running = false;
                        tui.render();
                    }
                }
            }
        }

        if !running {
            break;
        }
    }

    tui.stop();
    println!("\n {}\n", "Goodbye!".with(Color::DarkGrey));
    Ok(())
}

// ── Helper: get mutable ref to the editor (index 3 in root) ────────

fn get_editor_mut(tui: &mut TUI) -> &mut Editor {
    tui.root.children[3]
        .as_any_mut()
        .downcast_mut::<Editor>()
        .expect("child[3] should be Editor")
}

// ── Helper: access chat container (index 1) ────────────────────────

fn add_user_to_chat(tui: &mut TUI, text: &str) {
    let mut lines = vec![
        format!(" {}", style_role_user()),
    ];
    for line in text.lines() {
        lines.push(format!("  {}", line));
    }
    lines.push(String::new());
    add_lines_to_chat(tui, lines);
}

fn add_assistant_header(tui: &mut TUI, model_id: &str) {
    let lines = vec![
        format!(" {}", style_role_assistant(model_id)),
    ];
    add_lines_to_chat(tui, lines);
}

fn add_status_to_chat(tui: &mut TUI, text: &str) {
    add_lines_to_chat(tui, vec![text.to_string(), String::new()]);
}

fn add_lines_to_chat(tui: &mut TUI, lines: Vec<String>) {
    let chat = tui.root.children[1]
        .as_any_mut()
        .downcast_mut::<Container>()
        .expect("child[1] should be Container");
    chat.add(Box::new(Text { lines }));
}

/// Streaming state for in-progress assistant text.
struct StreamingState {
    text: String,
    component_idx: Option<usize>,
}

impl StreamingState {
    fn new() -> Self {
        Self {
            text: String::new(),
            component_idx: None,
        }
    }

    fn append(&mut self, tui: &mut TUI, delta: &str) {
        if self.component_idx.is_none() {
            self.text.clear();
            let container = tui.root.children[1]
                .as_any_mut()
                .downcast_mut::<Container>()
                .expect("child[1] is Container");
            let idx = container.len();
            container.add(Box::new(Text::new("")));
            self.component_idx = Some(idx);
        }

        self.text.push_str(delta);

        if let Some(idx) = self.component_idx {
            let container = tui.root.children[1]
                .as_any_mut()
                .downcast_mut::<Container>()
                .expect("child[1] is Container");
            if let Some(child) = container.children.get_mut(idx) {
                let text_comp = child
                    .as_any_mut()
                    .downcast_mut::<Text>()
                    .expect("streaming child is Text");
                let mut lines: Vec<String> = self.text.lines().map(|l| format!("  {}", l)).collect();
                if lines.is_empty() {
                    lines.push("  ".to_string());
                }
                text_comp.lines = lines;
            }
        }
    }

    fn finalize(&mut self, tui: &mut TUI, full_text: &str) {
        if let Some(idx) = self.component_idx.take() {
            let width = tui.columns().saturating_sub(4);
            let mut renderer = MarkdownRenderer::new(full_text);
            let md_lines = renderer.render(width);
            let mut lines: Vec<String> = md_lines.into_iter().map(|l| format!("  {}", l)).collect();
            lines.push(String::new());

            let container = tui.root.children[1]
                .as_any_mut()
                .downcast_mut::<Container>()
                .expect("child[1] is Container");
            if let Some(child) = container.children.get_mut(idx) {
                let text_comp = child
                    .as_any_mut()
                    .downcast_mut::<Text>()
                    .expect("streaming child is Text");
                text_comp.lines = lines;
            }
        }
        self.text.clear();
    }
}

// ── Footer update ──────────────────────────────────────────────────

fn update_footer(
    tui: &mut TUI,
    _model: &bb_provider::registry::Model,
    _cwd: &str,
    input_tokens: u64,
    output_tokens: u64,
    cost: f64,
) {
    let footer = tui.root.children[4]
        .as_any_mut()
        .downcast_mut::<Footer>()
        .expect("child[4] is Footer");
    footer.data.input_tokens = input_tokens;
    footer.data.output_tokens = output_tokens;
    footer.data.cost = cost;
}

// ── Append user message to session store ────────────────────────────

fn append_user_message(
    conn: &rusqlite::Connection,
    session_id: &str,
    text: &str,
) -> Result<()> {
    let entry = SessionEntry::Message {
        base: EntryBase {
            id: EntryId::generate(),
            parent_id: get_leaf(conn, session_id),
            timestamp: Utc::now(),
        },
        message: AgentMessage::User(UserMessage {
            content: vec![ContentBlock::Text { text: text.to_string() }],
            timestamp: Utc::now().timestamp_millis(),
        }),
    };
    store::append_entry(conn, session_id, &entry)?;
    Ok(())
}

fn get_leaf(conn: &rusqlite::Connection, session_id: &str) -> Option<EntryId> {
    store::get_session(conn, session_id)
        .ok()
        .flatten()
        .and_then(|s| s.leaf_id.map(EntryId))
}

fn conn_path(global_dir: &std::path::Path) -> std::path::PathBuf {
    global_dir.join("sessions.db")
}

// ── Spawn agent turn in background ──────────────────────────────────

fn spawn_agent_turn(
    db_path: std::path::PathBuf,
    session_id: String,
    system_prompt: String,
    model: bb_provider::registry::Model,
    provider_name: String,
    api_key: String,
    base_url: String,
    tool_defs: Vec<serde_json::Value>,
    thinking: Option<String>,
    agent_tx: mpsc::UnboundedSender<AgentEvent>,
    cancel: CancellationToken,
) {
    tokio::spawn(async move {
        if let Err(e) = run_agent_turn(
            &db_path, &session_id, &system_prompt, &model,
            &provider_name, &api_key, &base_url, &tool_defs,
            thinking.as_deref(), &agent_tx, &cancel,
        ).await {
            let _ = agent_tx.send(AgentEvent::Error(e.to_string()));
        }
    });
}

async fn run_agent_turn(
    db_path: &std::path::Path,
    session_id: &str,
    system_prompt: &str,
    model: &bb_provider::registry::Model,
    provider_name: &str,
    api_key: &str,
    base_url: &str,
    tool_defs: &[serde_json::Value],
    thinking: Option<&str>,
    agent_tx: &mpsc::UnboundedSender<AgentEvent>,
    cancel: &CancellationToken,
) -> Result<()> {
    let conn = store::open_db(db_path)?;
    let tools = builtin_tools();
    let cwd = store::get_session(&conn, session_id)?
        .map(|s| std::path::PathBuf::from(&s.cwd))
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let artifacts_dir = config::global_dir().join("artifacts");
    let tool_ctx = ToolContext {
        cwd: cwd.clone(),
        artifacts_dir,
        on_output: None,
    };

    let provider: Box<dyn Provider> = match model.api {
        ApiType::AnthropicMessages => Box::new(AnthropicProvider::new()),
        _ => Box::new(OpenAiProvider::new()),
    };

    // Agent loop (tool use may require multiple turns)
    loop {
        if cancel.is_cancelled() {
            let _ = agent_tx.send(AgentEvent::Done);
            return Ok(());
        }

        let ctx = context::build_context(&conn, session_id)?;
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
            cancel: cancel.clone(),
        };

        let (tx, mut rx) = mpsc::unbounded_channel();

        // Send assistant header
        let _ = agent_tx.send(AgentEvent::TextDelta(String::new()));

        let stream_result = provider.stream(request, options, tx).await;
        if let Err(e) = stream_result {
            let _ = agent_tx.send(AgentEvent::Error(e.to_string()));
            return Ok(());
        }

        let mut all_events = Vec::new();
        while let Some(event) = rx.recv().await {
            if cancel.is_cancelled() {
                let _ = agent_tx.send(AgentEvent::Done);
                return Ok(());
            }

            match &event {
                StreamEvent::TextDelta { text } => {
                    let _ = agent_tx.send(AgentEvent::TextDelta(text.clone()));
                }
                StreamEvent::ThinkingDelta { text } => {
                    let _ = agent_tx.send(AgentEvent::ThinkingDelta(text.clone()));
                }
                StreamEvent::ToolCallStart { name, id } => {
                    let _ = agent_tx.send(AgentEvent::ToolStart {
                        name: name.clone(),
                        id: id.clone(),
                    });
                }
                StreamEvent::Error { message } => {
                    let _ = agent_tx.send(AgentEvent::Error(message.clone()));
                }
                _ => {}
            }
            all_events.push(event);
        }

        let collected = bb_provider::streaming::CollectedResponse::from_events(&all_events);

        let _ = agent_tx.send(AgentEvent::TurnComplete {
            text: collected.text.clone(),
            has_tool_calls: !collected.tool_calls.is_empty(),
            input_tokens: collected.input_tokens,
            output_tokens: collected.output_tokens,
        });

        // Store assistant message
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
                parent_id: get_leaf(&conn, session_id),
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
        store::append_entry(&conn, session_id, &asst_entry)?;

        if collected.tool_calls.is_empty() {
            let _ = agent_tx.send(AgentEvent::Done);
            break;
        }

        // Execute tools
        for tc in &collected.tool_calls {
            if cancel.is_cancelled() {
                let _ = agent_tx.send(AgentEvent::Done);
                return Ok(());
            }

            let args: serde_json::Value =
                serde_json::from_str(&tc.arguments).unwrap_or(serde_json::json!({}));

            let tool = tools.iter().find(|t| t.name() == tc.name);
            let result = match tool {
                Some(t) => t.execute(args, &tool_ctx, cancel.clone()).await,
                None => Err(bb_core::error::BbError::Tool(format!("Unknown tool: {}", tc.name))),
            };

            let (tool_content, is_error, preview) = match result {
                Ok(r) => {
                    let preview = r.content.iter()
                        .filter_map(|b| if let ContentBlock::Text { text } = b { Some(text.lines().take(3).collect::<Vec<_>>().join(" ")) } else { None })
                        .collect::<Vec<_>>()
                        .join(" ");
                    let preview = if preview.len() > 80 { format!("{}...", &preview[..80]) } else { preview };
                    (r.content, r.is_error, preview)
                }
                Err(e) => {
                    let msg = format!("Error: {e}");
                    (vec![ContentBlock::Text { text: msg.clone() }], true, msg)
                }
            };

            let _ = agent_tx.send(AgentEvent::ToolResult {
                name: tc.name.clone(),
                success: !is_error,
                preview,
            });

            let tr_entry = SessionEntry::Message {
                base: EntryBase {
                    id: EntryId::generate(),
                    parent_id: get_leaf(&conn, session_id),
                    timestamp: Utc::now(),
                },
                message: AgentMessage::ToolResult(ToolResultMessage {
                    tool_call_id: tc.id.clone(),
                    tool_name: tc.name.clone(),
                    content: tool_content,
                    details: None,
                    is_error,
                    timestamp: Utc::now().timestamp_millis(),
                }),
            };
            store::append_entry(&conn, session_id, &tr_entry)?;
        }

        // Continue loop for next agent turn
    }

    Ok(())
}

// ── Bash shortcut ───────────────────────────────────────────────────

fn handle_bash(input: &str, cwd: &std::path::Path, tui: &mut TUI) {
    let cmd = input
        .strip_prefix("!!")
        .or_else(|| input.strip_prefix('!'))
        .unwrap_or("")
        .trim();
    if cmd.is_empty() {
        return;
    }
    let mut lines = vec![
        format!("  {} $ {}", dim("!"), cmd),
    ];
    match std::process::Command::new("bash")
        .arg("-c")
        .arg(cmd)
        .current_dir(cwd)
        .output()
    {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            for line in stdout.lines().take(20) {
                lines.push(format!("  {}", line));
            }
            if stdout.lines().count() > 20 {
                lines.push(format!("  {}", dim(&format!("[{} more lines]", stdout.lines().count() - 20))));
            }
            for line in stderr.lines() {
                lines.push(format!("  {}", style_error(line)));
            }
        }
        Err(e) => {
            lines.push(format!("  {}", style_error(&format!("bash error: {e}"))));
        }
    }
    lines.push(String::new());
    add_lines_to_chat(tui, lines);
}

// ── Slash command handling ──────────────────────────────────────────

async fn handle_slash(
    input: &str,
    conn: &rusqlite::Connection,
    session_id: &str,
    cwd_str: &str,
) -> Result<bool> {
    match crate::slash::handle_slash_command(input) {
        SlashResult::Exit => return Ok(true),
        SlashResult::Handled => {}
        SlashResult::NewSession => {
            println!("  Start a new `bb` to get a fresh session.");
        }
        SlashResult::Compact(_) => {}
        SlashResult::ModelSelect(_) => {
            crate::models::list_models(None);
        }
        SlashResult::Resume => {
            let sessions = store::list_sessions(conn, cwd_str)?;
            if sessions.is_empty() {
                println!("  No sessions.");
            } else {
                for (i, s) in sessions.iter().take(10).enumerate() {
                    let name = s.name.as_deref().unwrap_or("(unnamed)");
                    println!("  {}. {} {} ({} entries)", i + 1, &s.session_id[..8], name, s.entry_count);
                }
            }
        }
        SlashResult::Tree | SlashResult::Fork => {}
        SlashResult::Login => {
            login::handle_login(None).await?;
        }
        SlashResult::Logout => {
            login::handle_logout(None).await?;
        }
        SlashResult::SetName(name) => {
            println!("  Session named: {name}");
        }
        SlashResult::SessionInfo => {
            if let Ok(Some(session)) = store::get_session(conn, session_id) {
                println!("  Session: {}", &session.session_id[..8]);
                println!("  CWD: {}", session.cwd);
                println!("  Entries: {}", session.entry_count);
            }
        }
        SlashResult::NotCommand => {}
    }
    Ok(false)
}
