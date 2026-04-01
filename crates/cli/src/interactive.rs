//! Interactive mode — scrollback-based TUI matching pi's visual style.
//!
//! Architecture (matching pi):
//! - Component tree: header → chat → editor → footer
//! - Async event loop polling keyboard events + agent response events
//! - Differential rendering with synchronized output
//! - Bordered editor box (not > prompt)
//! - Real footer with cost/tokens/model

use anyhow::Result;

use bb_core::agent::{self, DEFAULT_SYSTEM_PROMPT};
use bb_core::config;
use bb_core::settings::Settings;
use bb_core::types::*;
use bb_provider::anthropic::AnthropicProvider;
use bb_provider::openai::OpenAiProvider;
use bb_provider::registry::{ApiType, ModelRegistry};
use bb_provider::{CompletionRequest, Provider, RequestOptions, StreamEvent};
use bb_session::{context, store};
use bb_tools::{builtin_tools, ToolContext};
use bb_tui::component::{Container, Focusable, Text};
use bb_tui::editor::Editor;
use bb_tui::footer::{Footer, FooterData};
use bb_tui::markdown::MarkdownRenderer;
use bb_tui::model_selector::ModelSelector;
use bb_tui::select_list::{SelectAction, SelectItem, SelectList};
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

fn title_case_provider(provider: &str) -> String {
    match provider {
        "openai" => "OpenAI".into(),
        "xai" => "xAI".into(),
        "openrouter" => "OpenRouter".into(),
        other => {
            let mut chars = other.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        }
    }
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
    /// Assistant response starting (show header).
    AssistantStart { model_id: String },
    /// Streaming text delta from assistant.
    TextDelta(String),
    /// Thinking indicator.
    #[allow(dead_code)]
    ThinkingDelta(String),
    /// Tool call started.
    ToolStart { name: String, #[allow(dead_code)] id: String },
    /// Tool execution result.
    ToolResult {
        name: String,
        success: bool,
        preview: String,
    },
    /// Assistant turn complete (text + tool calls collected).
    TurnComplete {
        text: String,
        #[allow(dead_code)] has_tool_calls: bool,
        input_tokens: u64,
        output_tokens: u64,
    },
    /// Need to continue (tool calls need execution then another turn).
    #[allow(dead_code)]
    NeedsContinue,
    /// Error.
    Error(String),
    /// Done — no more turns needed.
    Done,
}

// ── Banner component ────────────────────────────────────────────────

fn make_header(_model_name: &str) -> Text {
    let lines = vec![
        String::new(),
        format!(" {} v{}", "bb-agent".with(Color::Cyan).bold(), env!("CARGO_PKG_VERSION")),
        format!(" {} {}", dim("escape"), dim("to interrupt")),
        format!(" {} {}", dim("ctrl+c"), dim("to clear")),
        format!(" {} {}", dim("ctrl+c twice"), dim("to exit")),
        format!(" {} {}", dim("ctrl+d"), dim("to exit (empty)")),
        format!(" {} {}", dim("/"), dim("for commands")),
        format!(" {} {}", dim("!"), dim("to run bash")),
        String::new(),
    ];
    Text { lines }
}

// ── Main interactive mode ───────────────────────────────────────────

#[allow(unused_assignments)]
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

    let settings = Settings::load_merged(&cwd);
    let model_input = cli.model.as_deref().or(settings.default_model.as_deref());
    let provider_input = cli.provider.as_deref().or(settings.default_provider.as_deref());
    let (mut provider_name, model_id, _) = crate::run::parse_model_arg(
        provider_input,
        model_input,
    );

    let agents_md = crate::run::load_agents_md(&cwd);
    let system_prompt = agent::build_system_prompt(
        cli.system_prompt.as_deref().unwrap_or(DEFAULT_SYSTEM_PROMPT),
        agents_md.as_deref(),
    );

    let mut registry = ModelRegistry::new();
    registry.load_custom_models(&settings);

    let session_ctx = context::build_context(&conn, &session_id).ok();
    if cli.r#continue {
        if let Some(ctx) = &session_ctx {
            if let Some(saved) = &ctx.model {
                provider_name = saved.provider.clone();
            }
        }
    }

    let effective_model_id = if cli.r#continue {
        session_ctx.as_ref().and_then(|c| c.model.as_ref().map(|m| m.model_id.clone())).unwrap_or(model_id)
    } else {
        model_id
    };

    let mut model = registry
        .find(&provider_name, &effective_model_id)
        .cloned()
        .or_else(|| registry.find_fuzzy(&effective_model_id, Some(&provider_name)).cloned())
        .or_else(|| registry.find_fuzzy(&effective_model_id, None).cloned())
        .unwrap_or_else(|| bb_provider::registry::Model {
            id: effective_model_id.clone(),
            name: effective_model_id.clone(),
            provider: provider_name.clone(),
            api: ApiType::OpenaiCompletions,
            context_window: 128_000,
            max_tokens: 16384,
            reasoning: false,
            base_url: None,
            cost: Default::default(),
        });

    let mut api_key = cli
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

    let mut base_url = model
        .base_url
        .clone()
        .unwrap_or_else(|| "https://api.openai.com/v1".into());

    let tools = builtin_tools();
    let _tool_ctx = ToolContext {
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

    let _provider: Box<dyn Provider> = match model.api {
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

    // Index 2: Overlay/status area above editor (used for model selector)
    tui.root.add(Box::new(Text::empty()));

    // Index 3: Editor (focused)
    let mut editor = Editor::new();
    editor.terminal_rows = tui.rows();
    <Editor as Focusable>::set_focused(&mut editor, true);
    tui.root.add(Box::new(editor));

    // Index 4: Footer
    let git_branch = bb_tui::footer::detect_git_branch(&cwd_str);
    let footer = Footer::new(FooterData {
        model_name: model.id.clone(),
        provider: model.provider.clone(),
        cwd: cwd_str.clone(),
        git_branch,
        context_window: model.context_window,
        thinking_level: if model.reasoning { Some("medium".into()) } else { None },
        available_provider_count: available_provider_count(),
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
    let total_cost: f64 = 0.0;
    let mut running = true;
    let mut agent_running = false;
    let mut cancel_token = CancellationToken::new();
    let mut streaming = StreamingState::new();
    let mut last_ctrl_c = std::time::Instant::now() - std::time::Duration::from_secs(10);
    let mut model_selector: Option<ModelSelector> = None;
    let mut provider_selector: Option<(bool, SelectList)> = None; // (is_login, list)
    let mut auth_dialog: Option<AuthDialog> = None;

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
                        // Active auth dialog captures input first
                        if let Some(dialog) = &mut auth_dialog {
                            match key.code {
                                KeyCode::Esc => {
                                    auth_dialog = None;
                                    clear_overlay(&mut tui);
                                    tui.render();
                                    continue;
                                }
                                KeyCode::Enter => {
                                    let provider = dialog.provider.clone();
                                    let key_value = dialog.key_input.trim().to_string();
                                    auth_dialog = None;
                                    clear_overlay(&mut tui);
                                    if key_value.is_empty() {
                                        tui.render();
                                        continue;
                                    }
                                    match login::save_api_key(&provider, &key_value) {
                                        Ok(()) => {
                                            if provider == provider_name {
                                                api_key = cli.api_key.clone().unwrap_or_else(|| login::resolve_api_key(&provider_name).unwrap_or_default());
                                            }
                                            update_footer(&mut tui, &model, &cwd_str, total_input_tokens, total_output_tokens, total_cost);
                                            let provider_label = title_case_provider(&provider);
                                            add_status_to_chat(&mut tui, &format!("Logged in to {}. Credentials saved to {}", provider_label, login::auth_path().display()));
                                        }
                                        Err(e) => {
                                            let provider_label = title_case_provider(&provider);
                                            add_status_to_chat(&mut tui, &style_error(&format!("Failed to login to {}: {}", provider_label, e)));
                                        }
                                    }
                                    tui.render();
                                    continue;
                                }
                                KeyCode::Backspace => {
                                    dialog.key_input.pop();
                                }
                                KeyCode::Char(c) if key.modifiers == KeyModifiers::NONE || key.modifiers == KeyModifiers::SHIFT => {
                                    dialog.key_input.push(c);
                                }
                                _ => {}
                            }
                            let cols = tui.columns();
                            let lines = render_auth_dialog(dialog, cols);
                            set_overlay_lines(&mut tui, lines);
                            tui.render();
                            continue;
                        }

                        // Active provider selector captures input first
                        if let Some((is_login, list)) = &mut provider_selector {
                            let login_mode = *is_login;
                            match list.handle_key(key) {
                                SelectAction::None => {
                                    let cols = tui.columns();
                                    let lines = render_provider_selector(login_mode, list, cols);
                                    set_overlay_lines(&mut tui, lines);
                                    tui.render();
                                    continue;
                                }
                                SelectAction::Cancelled => {
                                    provider_selector = None;
                                    clear_overlay(&mut tui);
                                    tui.render();
                                    continue;
                                }
                                SelectAction::Selected(provider) => {
                                    provider_selector = None;
                                    clear_overlay(&mut tui);
                                    if login_mode {
                                        let dialog = make_auth_dialog(provider.clone());
                                        let cols = tui.columns();
                                        let lines = render_auth_dialog(&dialog, cols);
                                        set_overlay_lines(&mut tui, lines);
                                        auth_dialog = Some(dialog);
                                    } else {
                                        match login::remove_auth(&provider) {
                                            Ok(true) => {
                                                if provider == provider_name && cli.api_key.is_none() {
                                                    api_key = login::resolve_api_key(&provider_name).unwrap_or_default();
                                                }
                                                update_footer(&mut tui, &model, &cwd_str, total_input_tokens, total_output_tokens, total_cost);
                                                let provider_label = title_case_provider(&provider);
                                                add_status_to_chat(&mut tui, &format!("Logged out of {}", provider_label));
                                            }
                                            Ok(false) => add_status_to_chat(&mut tui, &style_error("No OAuth providers logged in. Use /login first.")),
                                            Err(e) => add_status_to_chat(&mut tui, &style_error(&format!("Logout failed: {}", e))),
                                        }
                                    }
                                    tui.render();
                                    continue;
                                }
                            }
                        }

                        // Active model selector captures input first
                        if let Some(selector) = &mut model_selector {
                            if let Some(result) = selector.handle_key(key) {
                                match result {
                                    Ok(selection) => {
                                        provider_name = selection.provider.clone();
                                        if let Some(found) = registry.find(&selection.provider, &selection.model_id).cloned() {
                                            model = found;
                                        } else {
                                            model.provider = selection.provider.clone();
                                            model.id = selection.model_id.clone();
                                            model.name = selection.name.clone();
                                            model.context_window = selection.context_window;
                                            model.reasoning = selection.reasoning;
                                        }
                                        api_key = cli.api_key.clone().unwrap_or_else(|| login::resolve_api_key(&provider_name).unwrap_or_default());
                                        base_url = model.base_url.clone().unwrap_or_else(|| "https://api.openai.com/v1".into());

                                        let model_change = SessionEntry::ModelChange {
                                            base: EntryBase {
                                                id: EntryId::generate(),
                                                parent_id: get_leaf(&conn, &session_id),
                                                timestamp: Utc::now(),
                                            },
                                            provider: model.provider.clone(),
                                            model_id: model.id.clone(),
                                        };
                                        let _ = store::append_entry(&conn, &session_id, &model_change);

                                        update_footer(&mut tui, &model, &cwd_str, total_input_tokens, total_output_tokens, total_cost);
                                        add_status_to_chat(&mut tui, &format!("  Switched to model: {} ({})", model.name, model.provider));
                                    }
                                    Err(()) => {}
                                }
                                model_selector = None;
                                clear_overlay(&mut tui);
                            } else {
                                let cols = tui.columns();
                                let lines = selector.render(cols);
                                set_overlay_lines(&mut tui, lines);
                            }
                            tui.render();
                            continue;
                        }

                        // Match specific key combinations

                        // Ctrl+D — exit if editor empty
                        if key.code == KeyCode::Char('d') && key.modifiers == KeyModifiers::CONTROL {
                            // Get editor text
                            let editor = get_editor_mut(&mut tui);
                            if editor.get_text().trim().is_empty() {
                                running = false;
                            }
                        }

                        // Escape — interrupt running agent
                        if key.code == KeyCode::Esc {
                            if agent_running {
                                cancel_token.cancel();
                                cancel_token = CancellationToken::new();
                                agent_running = false;
                                streaming.finalize(&mut tui, &streaming.text.clone());
                                add_status_to_chat(&mut tui, &dim("[interrupted]"));
                            }
                            tui.render();
                            continue;
                        }

                        // Ctrl+C — cancel agent, clear editor, or exit (double press)
                        if key.code == KeyCode::Char('c') && key.modifiers == KeyModifiers::CONTROL {
                            if agent_running {
                                cancel_token.cancel();
                                cancel_token = CancellationToken::new();
                                agent_running = false;
                                streaming.finalize(&mut tui, &streaming.text.clone());
                                add_status_to_chat(&mut tui, &dim("[cancelled]"));
                            } else {
                                let editor = get_editor_mut(&mut tui);
                                if editor.get_text().trim().is_empty() {
                                    // Double Ctrl+C on empty editor = exit
                                    if last_ctrl_c.elapsed() < std::time::Duration::from_millis(500) {
                                        running = false;
                                        break;
                                    }
                                }
                                editor.clear();
                            }
                            last_ctrl_c = std::time::Instant::now();
                            tui.render();
                            continue;
                        }

                        // Enter — accept slash menu or submit if not during agent turn
                        if key.code == KeyCode::Enter && key.modifiers == KeyModifiers::NONE && !agent_running {
                            let showing_slash_menu = {
                                let editor = get_editor_mut(&mut tui);
                                editor.is_showing_slash_menu()
                            };

                            if showing_slash_menu {
                                tui.handle_key(&key);
                                let accepted_text = {
                                    let editor = get_editor_mut(&mut tui);
                                    if !editor.is_showing_slash_menu() {
                                        editor.get_text()
                                    } else {
                                        String::new()
                                    }
                                };
                                if accepted_text == "/model" {
                                    get_editor_mut(&mut tui).clear();
                                    let selector = model_selector_from_registry(&registry, &provider_name, &model);
                                    let cols = tui.columns();
                                    let lines = selector.render(cols);
                                    set_overlay_lines(&mut tui, lines);
                                    model_selector = Some(selector);
                                } else if accepted_text == "/login" {
                                    get_editor_mut(&mut tui).clear();
                                    let list = provider_selector_list(true);
                                    let cols = tui.columns();
                                    let lines = render_provider_selector(true, &list, cols);
                                    set_overlay_lines(&mut tui, lines);
                                    provider_selector = Some((true, list));
                                } else if accepted_text == "/logout" {
                                    get_editor_mut(&mut tui).clear();
                                    let list = provider_selector_list(false);
                                    if login::list_known_providers().into_iter().all(|(_, configured)| !configured) {
                                        add_status_to_chat(&mut tui, "No OAuth providers logged in. Use /login first.");
                                    } else {
                                        let cols = tui.columns();
                                        let lines = render_provider_selector(false, &list, cols);
                                        set_overlay_lines(&mut tui, lines);
                                        provider_selector = Some((false, list));
                                    }
                                }
                                tui.render();
                                continue;
                            }

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
                                    if text == "/model" || text.starts_with("/model ") {
                                        let mut selector = model_selector_from_registry(&registry, &provider_name, &model);
                                        if let Some(query) = text.strip_prefix("/model ") {
                                            selector.set_search(query.trim());
                                        }
                                        let cols = tui.columns();
                                        let lines = selector.render(cols);
                                        set_overlay_lines(&mut tui, lines);
                                        model_selector = Some(selector);
                                        tui.render();
                                        continue;
                                    }

                                    if text == "/login" {
                                        get_editor_mut(&mut tui).clear();
                                        let list = provider_selector_list(true);
                                        let cols = tui.columns();
                                        let lines = render_provider_selector(true, &list, cols);
                                        set_overlay_lines(&mut tui, lines);
                                        provider_selector = Some((true, list));
                                        tui.render();
                                        continue;
                                    }

                                    if text == "/logout" {
                                        get_editor_mut(&mut tui).clear();
                                        let list = provider_selector_list(false);
                                        if login::list_known_providers().into_iter().all(|(_, configured)| !configured) {
                                            add_status_to_chat(&mut tui, "No OAuth providers logged in. Use /login first.");
                                        } else {
                                            let cols = tui.columns();
                                            let lines = render_provider_selector(false, &list, cols);
                                            set_overlay_lines(&mut tui, lines);
                                            provider_selector = Some((false, list));
                                        }
                                        tui.render();
                                        continue;
                                    }

                                    match handle_slash(&text, &conn, &session_id, &cwd_str).await {
                                        Ok((true, _)) => {
                                            running = false;
                                            break;
                                        }
                                        Ok((false, lines)) => {
                                            if !lines.is_empty() {
                                                let mut out = lines;
                                                out.push(String::new());
                                                add_lines_to_chat(&mut tui, out);
                                            }
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
                    AgentEvent::AssistantStart { model_id } => {
                        add_lines_to_chat(&mut tui, vec![
                            format!(" {}", style_role_assistant(&model_id)),
                        ]);
                        tui.render();
                    }

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


fn add_status_to_chat(tui: &mut TUI, text: &str) {
    add_lines_to_chat(tui, vec![text.to_string(), String::new()]);
}

fn set_overlay_lines(tui: &mut TUI, lines: Vec<String>) {
    let overlay = tui.root.children[2]
        .as_any_mut()
        .downcast_mut::<Text>()
        .expect("child[2] should be Text");
    overlay.lines = lines;
}

fn clear_overlay(tui: &mut TUI) {
    set_overlay_lines(tui, Vec::new());
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
    pub text: String,
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

struct AuthDialog {
    provider: String,
    key_input: String,
    env_var: String,
    url: String,
    has_env_key: bool,
}

fn provider_selector_list(is_login: bool) -> SelectList {
    let mut items: Vec<SelectItem> = login::list_known_providers()
        .into_iter()
        .filter(|(_, configured)| is_login || *configured)
        .map(|(name, configured)| SelectItem {
            label: name.clone(),
            detail: Some(if configured { "✓ logged in".into() } else { String::new() }),
            value: name,
        })
        .collect();
    items.sort_by(|a, b| a.label.cmp(&b.label));
    let mut list = SelectList::new(items, 8);
    list.set_show_search(false);
    list
}

fn render_provider_selector(is_login: bool, list: &SelectList, width: u16) -> Vec<String> {
    let title = if is_login { "Select provider to login:" } else { "Select provider to logout:" };
    let mut lines = vec![title.to_string(), String::new()];
    lines.extend(list.render(width));
    lines
}

fn make_auth_dialog(provider: String) -> AuthDialog {
    let (env_var, url) = login::provider_meta(&provider);
    let env_var = env_var.to_string();
    let url = url.to_string();
    let has_env_key = login::provider_has_env_key(&provider);
    AuthDialog {
        provider,
        key_input: String::new(),
        env_var,
        url,
        has_env_key,
    }
}

fn render_auth_dialog(dialog: &AuthDialog, width: u16) -> Vec<String> {
    let mut lines = vec![
        format!("Login to {}", dialog.provider),
        String::new(),
    ];
    if !dialog.url.is_empty() {
        lines.push(format!("Get your API key from: {}", dialog.url));
    }
    lines.push(format!("Tip: you can also set {} in the environment", dialog.env_var));
    if dialog.has_env_key {
        lines.push(format!("{} is already set in the environment; saved key will override bb auth storage", dialog.env_var));
    }
    lines.push(String::new());
    lines.push(format!("Enter API key for {}:", dialog.provider));
    lines.push(format!("> {}", "*".repeat(dialog.key_input.chars().count())));
    lines.push(String::new());
    lines.push("Esc to cancel • Enter to save".to_string());
    lines.into_iter().map(|line| bb_tui::utils::truncate_to_width(&line, width as usize)).collect()
}

fn model_selector_from_registry(
    registry: &ModelRegistry,
    current_provider: &str,
    current_model: &bb_provider::registry::Model,
) -> ModelSelector {
    let mut models: Vec<bb_provider::registry::Model> = registry
        .list()
        .iter()
        .filter(|m| login::resolve_api_key(&m.provider).map(|k| !k.is_empty()).unwrap_or(false))
        .cloned()
        .collect();

    if models.is_empty() {
        models = registry.list().to_vec();
    }

    models.sort_by(|a, b| {
        let a_current = a.provider == current_provider && a.id == current_model.id;
        let b_current = b.provider == current_provider && b.id == current_model.id;
        b_current.cmp(&a_current)
            .then_with(|| a.provider.cmp(&b.provider))
            .then_with(|| a.id.cmp(&b.id))
    });

    ModelSelector::from_models(models, 8)
}

fn available_provider_count() -> usize {
    let count = login::list_known_providers()
        .into_iter()
        .filter(|(_, configured)| *configured)
        .count();
    count.max(1)
}

fn update_footer(
    tui: &mut TUI,
    model: &bb_provider::registry::Model,
    cwd: &str,
    input_tokens: u64,
    output_tokens: u64,
    cost: f64,
) {
    let footer = tui.root.children[4]
        .as_any_mut()
        .downcast_mut::<Footer>()
        .expect("child[4] is Footer");
    footer.data.model_name = model.id.clone();
    footer.data.provider = model.provider.clone();
    footer.data.cwd = cwd.to_string();
    footer.data.context_window = model.context_window;
    footer.data.thinking_level = if model.reasoning { Some("medium".into()) } else { None };
    footer.data.available_provider_count = available_provider_count();
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
    _provider_name: &str,
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

        // Send assistant header before streaming starts
        let _ = agent_tx.send(AgentEvent::AssistantStart {
            model_id: model.id.clone(),
        });

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

/// Handle slash commands, returning (should_exit, output_lines).
async fn handle_slash(
    input: &str,
    conn: &rusqlite::Connection,
    session_id: &str,
    cwd_str: &str,
) -> Result<(bool, Vec<String>)> {
    let mut output = Vec::new();
    match slash::handle_slash_command(input) {
        SlashResult::Exit => return Ok((true, output)),
        SlashResult::Help => {
            output = slash::help_lines();
        }
        SlashResult::Handled => {}
        SlashResult::NewSession => {
            output.push("  Start a new `bb` to get a fresh session.".into());
        }
        SlashResult::Compact(_) => {
            output.push(dim("  (compaction not yet wired)"));
        }
        SlashResult::ModelSelect(_) => {
            output.push(dim("  Use --model to set model"));
        }
        SlashResult::Resume => {
            let sessions = store::list_sessions(conn, cwd_str)?;
            if sessions.is_empty() {
                output.push("  No sessions.".into());
            } else {
                for (i, s) in sessions.iter().take(10).enumerate() {
                    let name = s.name.as_deref().unwrap_or("(unnamed)");
                    output.push(format!("  {}. {} {} ({} entries)", i + 1, &s.session_id[..8], name, s.entry_count));
                }
            }
        }
        SlashResult::Tree | SlashResult::Fork => {
            output.push(dim("  (not yet implemented)"));
        }
        SlashResult::Login => {}
        SlashResult::Logout => {}
        SlashResult::SetName(name) => {
            output.push(format!("  Session named: {name}"));
        }
        SlashResult::SessionInfo => {
            if let Ok(Some(session)) = store::get_session(conn, session_id) {
                output.push(format!("  Session: {}", &session.session_id[..8]));
                output.push(format!("  CWD: {}", session.cwd));
                output.push(format!("  Entries: {}", session.entry_count));
            }
        }
        SlashResult::NotCommand => {
            output.push(dim(&format!("  Unknown command: {}", input)));
            output.push(dim("  Type /help for available commands"));
        }
    }
    Ok((false, output))
}
