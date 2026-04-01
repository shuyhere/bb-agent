#![allow(dead_code)]

pub mod components;
#[path = "../interactive_events.rs"]
pub mod events;
#[path = "../interactive_commands.rs"]
pub mod interactive_commands;

use self::events::{
    ChatItem, InteractiveMessage, InteractiveRenderState, PendingMessages,
    QueuedMessage as RenderQueuedMessage, QueuedMessageMode, ToolCallContent,
    assistant_message_from_parts,
};
use self::interactive_commands::InteractiveCommands;
use bb_core::agent_loop::AgentLoopEvent;
use bb_core::agent_session::{ModelRef, PromptOptions, ThinkingLevel};
use bb_core::agent_session_runtime::{AgentSessionRuntimeHost, RuntimeModelRef};
use bb_provider::registry::{ApiType, Model, ModelRegistry};
use bb_provider::Provider;
use bb_session::{compaction, store};
use bb_tools::{Tool, ToolContext};
use bb_tui::component::{Component, Container, Focusable, Spacer, Text};
use bb_tui::editor::Editor;
use bb_tui::model_selector::{ModelSelection, ModelSelector};
use bb_tui::terminal::{Terminal, TerminalEvent};
use bb_tui::tui_core::TUI;
use bb_tui::utils::word_wrap;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rusqlite::params;
use std::any::Any;
use std::collections::VecDeque;
use std::error::Error;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio::sync::mpsc::UnboundedReceiver;

pub type InteractiveResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

#[derive(Clone, Debug, Default)]
pub struct InteractiveModeOptions {
    pub verbose: bool,
    pub quiet_startup: bool,
    pub migrated_providers: Vec<String>,
    pub model_fallback_message: Option<String>,
    pub initial_message: Option<String>,
    pub initial_images: Vec<String>,
    pub initial_messages: Vec<String>,
    pub session_id: Option<String>,
    pub model_display: Option<String>,
    pub agents_md: Option<String>,
}

/// Non-Clone runtime state needed for actual LLM calls.
pub struct InteractiveSessionSetup {
    pub conn: rusqlite::Connection,
    pub session_id: String,
    pub provider: Box<dyn Provider>,
    pub model: bb_provider::registry::Model,
    pub api_key: String,
    pub base_url: String,
    pub tools: Vec<Box<dyn Tool>>,
    pub tool_defs: Vec<serde_json::Value>,
    pub tool_ctx: ToolContext,
    pub system_prompt: String,
    pub thinking_level: String,
}

#[derive(Debug, Default)]
struct InteractiveSessionState {
    render_state: InteractiveRenderState,
    pending_messages: PendingMessages,
}

enum ModelSelectorOverlayAction {
    Selected(ModelSelection),
    Cancelled,
}

struct ModelSelectorOverlay {
    selector: ModelSelector,
    current_model: String,
    initial_search: Option<String>,
    pending_action: Option<ModelSelectorOverlayAction>,
}

impl ModelSelectorOverlay {
    fn new(selector: ModelSelector, current_model: String, initial_search: Option<String>) -> Self {
        Self {
            selector,
            current_model,
            initial_search,
            pending_action: None,
        }
    }

    fn take_action(&mut self) -> Option<ModelSelectorOverlayAction> {
        self.pending_action.take()
    }
}

impl Component for ModelSelectorOverlay {
    fn render(&self, width: u16) -> Vec<String> {
        let purple = "\x1b[38;2;178;148;187m";
        let dim = "\x1b[90m";
        let reset = "\x1b[0m";
        let inner_width = width.saturating_sub(2);
        let border = format!("{purple}{}{reset}", "─".repeat(width.max(1) as usize));
        let mut lines = vec![
            border.clone(),
            format!(" {purple}Select model{reset}"),
            format!(" {dim}Current: {}{reset}", self.current_model),
        ];
        if let Some(search) = self.initial_search.as_deref().filter(|s| !s.is_empty()) {
            lines.push(format!(" {dim}Search: {search}{reset}"));
        } else {
            lines.push(format!(" {dim}Type to search · Enter select · Esc cancel{reset}"));
        }
        lines.push(String::new());
        lines.extend(
            self.selector
                .render(inner_width)
                .into_iter()
                .map(|line| format!(" {line}")),
        );
        lines.push(border);
        lines
    }

    fn handle_input(&mut self, key: &KeyEvent) {
        match self.selector.handle_key(*key) {
            Some(Ok(selection)) => self.pending_action = Some(ModelSelectorOverlayAction::Selected(selection)),
            Some(Err(())) => self.pending_action = Some(ModelSelectorOverlayAction::Cancelled),
            None => {}
        }
    }

    fn invalidate(&mut self) {}

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

struct InteractiveController {
    runtime_host: AgentSessionRuntimeHost,
    session: InteractiveSessionState,
    commands: InteractiveCommands,
}

impl InteractiveController {
    fn new(runtime_host: AgentSessionRuntimeHost) -> Self {
        Self {
            runtime_host,
            session: InteractiveSessionState::default(),
            commands: InteractiveCommands::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct KeyBinding {
    code: KeyCode,
    modifiers: KeyModifiers,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum KeyAction {
    Escape,
    ClearOrInterrupt,
    ExitEmpty,
    Suspend,
    CycleThinking,
    CycleModelForward,
    CycleModelBackward,
    SelectModel,
    ToggleToolExpansion,
    ToggleThinkingVisibility,
    OpenExternalEditor,
    FollowUp,
    Dequeue,
    SessionNew,
    SessionTree,
    SessionFork,
    SessionResume,
    PasteImage,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SubmitAction {
    Settings,
    ScopedModels,
    Model,
    Export,
    Import,
    Share,
    Copy,
    Name,
    Session,
    Changelog,
    Hotkeys,
    Fork,
    Tree,
    Login,
    Logout,
    New,
    Compact,
    Reload,
    Debug,
    ArminSaysHi,
    Resume,
    Quit,
    Help,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SubmitMatch {
    Exact(&'static str),
    Prefix(&'static str),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SubmitRoute {
    matcher: SubmitMatch,
    action: SubmitAction,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SubmitOutcome {
    Ignored,
    Submitted,
    Shutdown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum QueuedMessageKind {
    Steer,
    FollowUp,
}

impl Default for QueuedMessageKind {
    fn default() -> Self {
        Self::Steer
    }
}

#[derive(Debug, Default)]
struct QueuedMessage {
    text: String,
    kind: QueuedMessageKind,
}



struct SharedContainer {
    inner: Arc<Mutex<Container>>,
}

impl SharedContainer {
    fn new(inner: Arc<Mutex<Container>>) -> Self {
        Self { inner }
    }
}

impl Component for SharedContainer {
    fn render(&self, width: u16) -> Vec<String> {
        self.inner
            .lock()
            .map(|inner| inner.render(width))
            .unwrap_or_else(|_| vec!["<container unavailable>".to_string()])
    }

    fn handle_input(&mut self, key: &KeyEvent) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.handle_input(key);
        }
    }

    fn handle_raw_input(&mut self, data: &str) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.handle_raw_input(data);
        }
    }

    fn invalidate(&mut self) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.invalidate();
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}



pub struct InteractiveMode {
    controller: InteractiveController,
    session_setup: InteractiveSessionSetup,
    ui: TUI,
    header_container: Arc<Mutex<Container>>,
    chat_container: Arc<Mutex<Container>>,
    pending_messages_container: Arc<Mutex<Container>>,
    status_container: Arc<Mutex<Container>>,
    widget_container_above: Arc<Mutex<Container>>,
    widget_container_below: Arc<Mutex<Container>>,
    footer_container: Arc<Mutex<Container>>,
    editor: Arc<Mutex<Editor>>,
    version: String,
    options: InteractiveModeOptions,
    is_initialized: bool,
    on_input_callback: Option<Box<dyn FnMut(String) + Send>>,
    loading_animation: bool,
    pending_working_message: Option<String>,
    default_working_message: &'static str,
    default_hidden_thinking_label: &'static str,
    hidden_thinking_label: String,
    last_sigint_time: Option<Instant>,
    last_escape_time: Option<Instant>,
    changelog_markdown: Option<String>,
    tool_output_expanded: bool,
    hide_thinking_block: bool,
    shutdown_requested: bool,
    is_bash_mode: Arc<Mutex<bool>>,
    is_bash_running: bool,
    is_streaming: bool,
    is_compacting: bool,
    streaming_text: String,
    streaming_thinking: String,
    streaming_tool_calls: Vec<ToolCallContent>,
    pending_bash_components: VecDeque<String>,
    steering_queue: VecDeque<String>,
    follow_up_queue: VecDeque<String>,
    compaction_queued_messages: VecDeque<QueuedMessage>,
    key_handlers: Vec<(KeyBinding, KeyAction)>,
    submit_routes: Vec<SubmitRoute>,
    agent_events: Option<mpsc::UnboundedReceiver<AgentLoopEvent>>,
    events: Option<UnboundedReceiver<TerminalEvent>>,
    header_lines: Vec<String>,
    chat_lines: Vec<String>,
    pending_lines: Vec<String>,
    status_lines: Vec<String>,
    footer_lines: Vec<String>,
    widgets_above_lines: Vec<String>,
    widgets_below_lines: Vec<String>,
}

impl InteractiveMode {
    pub fn new(runtime_host: AgentSessionRuntimeHost, options: InteractiveModeOptions, session_setup: InteractiveSessionSetup) -> Self {
        let editor = {
            let mut e = Editor::new();
            e.set_focused(true);
            Arc::new(Mutex::new(e))
        };
        let is_bash_mode = Arc::new(Mutex::new(false));

        let mut this = Self {
            controller: InteractiveController::new(runtime_host),
            session_setup,
            ui: TUI::new(),
            header_container: Arc::new(Mutex::new(Container::new())),
            chat_container: Arc::new(Mutex::new(Container::new())),
            pending_messages_container: Arc::new(Mutex::new(Container::new())),
            status_container: Arc::new(Mutex::new(Container::new())),
            widget_container_above: Arc::new(Mutex::new(Container::new())),
            widget_container_below: Arc::new(Mutex::new(Container::new())),
            footer_container: Arc::new(Mutex::new(Container::new())),
            editor,
            version: env!("CARGO_PKG_VERSION").to_string(),
            options,
            is_initialized: false,
            on_input_callback: None,
            loading_animation: false,
            pending_working_message: None,
            default_working_message: "Working...",
            default_hidden_thinking_label: "Thinking...",
            hidden_thinking_label: "Thinking...".to_string(),
            last_sigint_time: None,
            last_escape_time: None,
            changelog_markdown: None,
            tool_output_expanded: false,
            hide_thinking_block: false,
            shutdown_requested: false,
            is_bash_mode,
            is_bash_running: false,
            is_streaming: false,
            is_compacting: false,
            streaming_text: String::new(),
            streaming_thinking: String::new(),
            streaming_tool_calls: Vec::new(),
            pending_bash_components: VecDeque::new(),
            steering_queue: VecDeque::new(),
            follow_up_queue: VecDeque::new(),
            compaction_queued_messages: VecDeque::new(),
            key_handlers: Vec::new(),
            submit_routes: Vec::new(),
            agent_events: None,
            events: None,
            header_lines: Vec::new(),
            chat_lines: Vec::new(),
            pending_lines: Vec::new(),
            status_lines: Vec::new(),
            footer_lines: Vec::new(),
            widgets_above_lines: Vec::new(),
            widgets_below_lines: Vec::new(),
        };
        this.render_widgets();
        this.rebuild_footer();
        this
    }

    fn render_state(&self) -> &InteractiveRenderState {
        &self.controller.session.render_state
    }

    fn render_state_mut(&mut self) -> &mut InteractiveRenderState {
        &mut self.controller.session.render_state
    }

    fn sync_pending_render_state(&mut self) {
        let queued = self
            .compaction_queued_messages
            .iter()
            .map(|message| RenderQueuedMessage {
                text: message.text.clone(),
                mode: match message.kind {
                    QueuedMessageKind::Steer => QueuedMessageMode::Steer,
                    QueuedMessageKind::FollowUp => QueuedMessageMode::FollowUp,
                },
            })
            .collect::<Vec<_>>();
        let steering: Vec<String> = self.steering_queue.iter().cloned().collect();
        let follow_up: Vec<String> = self.follow_up_queue.iter().cloned().collect();
        let pending = InteractiveRenderState::collect_pending_messages(&steering, &follow_up, &queued);
        self.controller.session.pending_messages = pending.clone();
        self.render_state_mut()
            .update_pending_messages_display(&pending);
    }

    fn render_items_to_lines(items: &[ChatItem], width: u16) -> Vec<String> {
        let dim = "\x1b[90m";
        let reset = "\x1b[0m";
        let content_width = width.saturating_sub(1).max(1) as usize;
        let wrap_prefixed = |line: &str| -> Vec<String> {
            if line.is_empty() {
                vec![String::new()]
            } else {
                word_wrap(line, content_width)
                    .into_iter()
                    .map(|l| format!(" {l}"))
                    .collect()
            }
        };

        items
            .iter()
            .flat_map(|item| match item {
                ChatItem::Spacer => vec![String::new()],
                ChatItem::UserMessage(text) => {
                    let user_bg = "\x1b[48;2;52;53;65m";
                    vec![String::new(), format!("{user_bg} {text}\x1b[K{reset}"), String::new()]
                }
                ChatItem::AssistantMessage(component) => component
                    .render_lines()
                    .iter()
                    .flat_map(|l| wrap_prefixed(l))
                    .collect(),
                ChatItem::ToolExecution(component) => component
                    .render_lines()
                    .iter()
                    .flat_map(|l| wrap_prefixed(l))
                    .collect(),
                ChatItem::BashExecution(component) => component
                    .render_lines()
                    .iter()
                    .flat_map(|l| wrap_prefixed(l))
                    .collect(),
                ChatItem::CustomMessage { text, .. } => word_wrap(&format!("{dim} {text}{reset}"), width.max(1) as usize),
                ChatItem::CompactionSummary(summary) => word_wrap(&format!("{dim} [c] {summary}{reset}"), width.max(1) as usize),
                ChatItem::BranchSummary(summary) => word_wrap(&format!("{dim} [b] {summary}{reset}"), width.max(1) as usize),
                ChatItem::PendingMessageLine(line) => wrap_prefixed(line),
            })
            .collect()
    }

    fn chat_render_lines(&self) -> Vec<String> {
        let width = self.ui.columns();
        let mut lines = Self::render_items_to_lines(&self.render_state().chat_items, width);
        for line in &self.chat_lines {
            lines.extend(word_wrap(line, width.max(1) as usize));
        }
        lines
    }

    fn pending_render_lines(&self) -> Vec<String> {
        let width = self.ui.columns();
        let mut lines = Self::render_items_to_lines(&self.render_state().pending_items, width);
        for line in &self.pending_lines {
            lines.extend(word_wrap(line, width.max(1) as usize));
        }
        lines
    }

    pub fn set_on_input_callback<F>(&mut self, callback: F)
    where
        F: FnMut(String) + Send + 'static,
    {
        self.on_input_callback = Some(Box::new(callback));
    }

    pub async fn init(&mut self) -> InteractiveResult<()> {
        if self.is_initialized {
            return Ok(());
        }

        self.changelog_markdown = self.get_changelog_for_display();

        self.ui.root.add(Box::new(SharedContainer::new(
            self.header_container.clone(),
        )));
        self.ui
            .root
            .add(Box::new(SharedContainer::new(self.chat_container.clone())));
        self.ui.root.add(Box::new(SharedContainer::new(
            self.pending_messages_container.clone(),
        )));
        self.ui.root.add(Box::new(SharedContainer::new(
            self.status_container.clone(),
        )));
        self.ui.root.add(Box::new(SharedContainer::new(
            self.widget_container_above.clone(),
        )));
        self.ui
            .root
            .add(Box::new(SharedEditorWrapper::new(self.editor.clone())));
        self.ui.root.add(Box::new(SharedContainer::new(
            self.widget_container_below.clone(),
        )));
        self.ui.root.add(Box::new(SharedContainer::new(
            self.footer_container.clone(),
        )));
        self.ui.set_focus(Some(5));

        self.rebuild_header();
        self.render_widgets();
        self.rebuild_footer();
        self.sync_static_sections();

        self.setup_key_handlers();
        self.setup_editor_submit_handler();

        self.events = Some(self.ui.start());
        self.is_initialized = true;

        self.bind_current_session_extensions().await?;
        self.render_initial_messages();
        self.update_terminal_title();
        self.refresh_ui();

        Ok(())
    }

    pub async fn run(&mut self) -> InteractiveResult<()> {
        self.init().await?;

        self.start_background_checks();

        if !self.options.migrated_providers.is_empty() {
            self.show_warning(format!(
                "Migrated credentials to auth.json: {}",
                self.options.migrated_providers.join(", ")
            ));
        }

        if let Some(message) = self.options.model_fallback_message.clone() {
            self.show_warning(message);
        }

        if let Some(initial_message) = self.options.initial_message.clone() {
            self.dispatch_prompt(initial_message).await?;
            self.drain_queued_messages().await?;
        }

        for message in self.options.initial_messages.clone() {
            self.dispatch_prompt(message).await?;
            self.drain_queued_messages().await?;
        }

        while !self.shutdown_requested {
            let Some(user_input) = self.get_user_input().await? else {
                break;
            };
            self.dispatch_prompt(user_input).await?;
            self.drain_queued_messages().await?;
        }

        self.stop_ui();
        Ok(())
    }

    /// Set the agent event receiver for streaming agent loop events.
    pub fn set_agent_events(&mut self, rx: UnboundedReceiver<AgentLoopEvent>) {
        self.agent_events = Some(rx);
    }

    async fn get_user_input(&mut self) -> InteractiveResult<Option<String>> {
        loop {
            if self.shutdown_requested {
                return Ok(None);
            }

            // Use tokio::select! to handle both terminal and agent events
            tokio::select! {
                terminal_event = async {
                    match self.events.as_mut() {
                        Some(events) => events.recv().await,
                        None => None,
                    }
                } => {
                    let Some(event) = terminal_event else {
                        self.shutdown_requested = true;
                        return Ok(None);
                    };

                    match event {
                        TerminalEvent::Resize(_, _) => {
                            self.ui.force_render();
                        }
                        TerminalEvent::Paste(data) | TerminalEvent::Raw(data) => {
                            self.ui.handle_raw_input(&data);
                            self.sync_bash_mode_from_editor();
                            self.refresh_ui();
                        }
                        TerminalEvent::Key(key) => {
                            if let Some(submitted) = self.handle_key_event(key).await? {
                                return Ok(Some(submitted));
                            }
                        }
                    }
                }
                agent_event = async {
                    match self.agent_events.as_mut() {
                        Some(rx) => rx.recv().await,
                        None => std::future::pending::<Option<AgentLoopEvent>>().await,
                    }
                } => {
                    if let Some(event) = agent_event {
                        self.handle_agent_event(event);
                    }
                }
            }
        }
    }

    async fn handle_key_event(&mut self, key: KeyEvent) -> InteractiveResult<Option<String>> {
        // Match pi: overlays own input while open.
        if self.ui.has_overlay() {
            self.ui.handle_key(&key);
            self.process_overlay_actions();
            self.refresh_ui();
            return Ok(None);
        }

        if let Some(action) = self.lookup_key_action(&key) {
            self.handle_key_action(action).await?;
            self.refresh_ui();
            return Ok(None);
        }

        if key.code == KeyCode::Enter && !key.modifiers.contains(KeyModifiers::SHIFT) {
            let text = self.editor_text();
            let outcome = self.handle_submitted_text(text).await?;
            self.refresh_ui();
            return match outcome {
                SubmitOutcome::Ignored => Ok(None),
                SubmitOutcome::Submitted => Ok(Some(self.take_last_submitted_text())),
                SubmitOutcome::Shutdown => Ok(None),
            };
        }

        self.ui.handle_key(&key);
        self.sync_bash_mode_from_editor();
        self.refresh_ui();
        Ok(None)
    }

    fn lookup_key_action(&self, key: &KeyEvent) -> Option<KeyAction> {
        self.key_handlers
            .iter()
            .find(|(binding, _)| binding.code == key.code && binding.modifiers == key.modifiers)
            .map(|(_, action)| *action)
    }

    fn setup_key_handlers(&mut self) {
        self.key_handlers.clear();
        self.key_handlers.push((
            KeyBinding {
                code: KeyCode::Esc,
                modifiers: KeyModifiers::NONE,
            },
            KeyAction::Escape,
        ));
        self.key_handlers.push((
            KeyBinding {
                code: KeyCode::Char('c'),
                modifiers: KeyModifiers::CONTROL,
            },
            KeyAction::ClearOrInterrupt,
        ));
        self.key_handlers.push((
            KeyBinding {
                code: KeyCode::Char('d'),
                modifiers: KeyModifiers::CONTROL,
            },
            KeyAction::ExitEmpty,
        ));
        self.key_handlers.push((
            KeyBinding {
                code: KeyCode::Char('z'),
                modifiers: KeyModifiers::CONTROL,
            },
            KeyAction::Suspend,
        ));
        self.key_handlers.push((
            KeyBinding {
                code: KeyCode::F(2),
                modifiers: KeyModifiers::NONE,
            },
            KeyAction::CycleThinking,
        ));
        self.key_handlers.push((
            KeyBinding {
                code: KeyCode::F(3),
                modifiers: KeyModifiers::NONE,
            },
            KeyAction::CycleModelForward,
        ));
        self.key_handlers.push((
            KeyBinding {
                code: KeyCode::F(4),
                modifiers: KeyModifiers::NONE,
            },
            KeyAction::CycleModelBackward,
        ));
        self.key_handlers.push((
            KeyBinding {
                code: KeyCode::F(5),
                modifiers: KeyModifiers::NONE,
            },
            KeyAction::SelectModel,
        ));
        self.key_handlers.push((
            KeyBinding {
                code: KeyCode::F(6),
                modifiers: KeyModifiers::NONE,
            },
            KeyAction::ToggleToolExpansion,
        ));
        self.key_handlers.push((
            KeyBinding {
                code: KeyCode::F(7),
                modifiers: KeyModifiers::NONE,
            },
            KeyAction::ToggleThinkingVisibility,
        ));
        self.key_handlers.push((
            KeyBinding {
                code: KeyCode::F(8),
                modifiers: KeyModifiers::NONE,
            },
            KeyAction::OpenExternalEditor,
        ));
        self.key_handlers.push((
            KeyBinding {
                code: KeyCode::F(9),
                modifiers: KeyModifiers::NONE,
            },
            KeyAction::FollowUp,
        ));
        self.key_handlers.push((
            KeyBinding {
                code: KeyCode::F(10),
                modifiers: KeyModifiers::NONE,
            },
            KeyAction::Dequeue,
        ));
        self.key_handlers.push((
            KeyBinding {
                code: KeyCode::F(11),
                modifiers: KeyModifiers::NONE,
            },
            KeyAction::SessionTree,
        ));
        self.key_handlers.push((
            KeyBinding {
                code: KeyCode::F(12),
                modifiers: KeyModifiers::NONE,
            },
            KeyAction::SessionResume,
        ));
        self.key_handlers.push((
            KeyBinding {
                code: KeyCode::Char('l'),
                modifiers: KeyModifiers::CONTROL,
            },
            KeyAction::SelectModel,
        ));
        self.key_handlers.push((
            KeyBinding {
                code: KeyCode::Char('p'),
                modifiers: KeyModifiers::CONTROL,
            },
            KeyAction::CycleModelForward,
        ));
        self.key_handlers.push((
            KeyBinding {
                code: KeyCode::Char('P'),
                modifiers: KeyModifiers::CONTROL | KeyModifiers::SHIFT,
            },
            KeyAction::CycleModelBackward,
        ));
        self.key_handlers.push((
            KeyBinding {
                code: KeyCode::Char('p'),
                modifiers: KeyModifiers::CONTROL | KeyModifiers::SHIFT,
            },
            KeyAction::CycleModelBackward,
        ));
    }

    fn setup_editor_submit_handler(&mut self) {
        self.submit_routes = vec![
            SubmitRoute {
                matcher: SubmitMatch::Exact("/settings"),
                action: SubmitAction::Settings,
            },
            SubmitRoute {
                matcher: SubmitMatch::Exact("/scoped-models"),
                action: SubmitAction::ScopedModels,
            },
            SubmitRoute {
                matcher: SubmitMatch::Exact("/model"),
                action: SubmitAction::Model,
            },
            SubmitRoute {
                matcher: SubmitMatch::Prefix("/model "),
                action: SubmitAction::Model,
            },
            SubmitRoute {
                matcher: SubmitMatch::Prefix("/export"),
                action: SubmitAction::Export,
            },
            SubmitRoute {
                matcher: SubmitMatch::Prefix("/import"),
                action: SubmitAction::Import,
            },
            SubmitRoute {
                matcher: SubmitMatch::Exact("/share"),
                action: SubmitAction::Share,
            },
            SubmitRoute {
                matcher: SubmitMatch::Exact("/copy"),
                action: SubmitAction::Copy,
            },
            SubmitRoute {
                matcher: SubmitMatch::Exact("/name"),
                action: SubmitAction::Name,
            },
            SubmitRoute {
                matcher: SubmitMatch::Prefix("/name "),
                action: SubmitAction::Name,
            },
            SubmitRoute {
                matcher: SubmitMatch::Exact("/session"),
                action: SubmitAction::Session,
            },
            SubmitRoute {
                matcher: SubmitMatch::Exact("/changelog"),
                action: SubmitAction::Changelog,
            },
            SubmitRoute {
                matcher: SubmitMatch::Exact("/hotkeys"),
                action: SubmitAction::Hotkeys,
            },
            SubmitRoute {
                matcher: SubmitMatch::Exact("/fork"),
                action: SubmitAction::Fork,
            },
            SubmitRoute {
                matcher: SubmitMatch::Exact("/tree"),
                action: SubmitAction::Tree,
            },
            SubmitRoute {
                matcher: SubmitMatch::Exact("/login"),
                action: SubmitAction::Login,
            },
            SubmitRoute {
                matcher: SubmitMatch::Exact("/logout"),
                action: SubmitAction::Logout,
            },
            SubmitRoute {
                matcher: SubmitMatch::Exact("/new"),
                action: SubmitAction::New,
            },
            SubmitRoute {
                matcher: SubmitMatch::Exact("/compact"),
                action: SubmitAction::Compact,
            },
            SubmitRoute {
                matcher: SubmitMatch::Prefix("/compact "),
                action: SubmitAction::Compact,
            },
            SubmitRoute {
                matcher: SubmitMatch::Exact("/reload"),
                action: SubmitAction::Reload,
            },
            SubmitRoute {
                matcher: SubmitMatch::Exact("/debug"),
                action: SubmitAction::Debug,
            },
            SubmitRoute {
                matcher: SubmitMatch::Exact("/arminsayshi"),
                action: SubmitAction::ArminSaysHi,
            },
            SubmitRoute {
                matcher: SubmitMatch::Exact("/resume"),
                action: SubmitAction::Resume,
            },
            SubmitRoute {
                matcher: SubmitMatch::Exact("/quit"),
                action: SubmitAction::Quit,
            },
            SubmitRoute {
                matcher: SubmitMatch::Exact("/help"),
                action: SubmitAction::Help,
            },
        ];
    }

    async fn handle_key_action(&mut self, action: KeyAction) -> InteractiveResult<()> {
        match action {
            KeyAction::Escape => self.handle_escape(),
            KeyAction::ClearOrInterrupt => self.handle_ctrl_c(),
            KeyAction::ExitEmpty => self.handle_ctrl_d(),
            KeyAction::Suspend => self.handle_ctrl_z(),
            KeyAction::CycleThinking => self.cycle_thinking_level(),
            KeyAction::CycleModelForward => self.cycle_model("forward"),
            KeyAction::CycleModelBackward => self.cycle_model("backward"),
            KeyAction::SelectModel => self.show_model_selector(None),
            KeyAction::ToggleToolExpansion => self.toggle_tool_output_expansion(),
            KeyAction::ToggleThinkingVisibility => self.toggle_thinking_block_visibility(),
            KeyAction::OpenExternalEditor => self.show_placeholder("external editor"),
            KeyAction::FollowUp => self.handle_follow_up(),
            KeyAction::Dequeue => self.handle_dequeue(),
            KeyAction::SessionNew => self.handle_new_session(),
            KeyAction::SessionTree => self.show_tree_selector(),
            KeyAction::SessionFork => self.show_user_message_selector(),
            KeyAction::SessionResume => self.show_session_selector(),
            KeyAction::PasteImage => self.handle_clipboard_image_paste(),
        }
        Ok(())
    }

    async fn handle_submitted_text(&mut self, text: String) -> InteractiveResult<SubmitOutcome> {
        let text = text.trim().to_string();
        if text.is_empty() {
            return Ok(SubmitOutcome::Ignored);
        }

        for route in &self.submit_routes {
            let matched = match route.matcher {
                SubmitMatch::Exact(command) => text == command,
                SubmitMatch::Prefix(prefix) => text.starts_with(prefix),
            };
            if !matched {
                continue;
            }

            match route.action {
                SubmitAction::Settings => {
                    self.show_settings_selector();
                    self.clear_editor();
                }
                SubmitAction::ScopedModels => {
                    self.clear_editor();
                    self.show_placeholder("scoped models selector");
                }
                SubmitAction::Model => {
                    let search = text
                        .strip_prefix("/model")
                        .map(str::trim)
                        .filter(|s| !s.is_empty());
                    self.clear_editor();
                    self.handle_model_command(search);
                }
                SubmitAction::Export => {
                    self.handle_export_command(&text);
                    self.clear_editor();
                }
                SubmitAction::Import => {
                    self.handle_import_command(&text);
                    self.clear_editor();
                }
                SubmitAction::Share => {
                    self.handle_share_command();
                    self.clear_editor();
                }
                SubmitAction::Copy => {
                    self.handle_copy_command();
                    self.clear_editor();
                }
                SubmitAction::Name => {
                    self.handle_name_command(&text);
                    self.clear_editor();
                }
                SubmitAction::Session => {
                    self.handle_session_command();
                    self.clear_editor();
                }
                SubmitAction::Changelog => {
                    self.handle_changelog_command();
                    self.clear_editor();
                }
                SubmitAction::Hotkeys => {
                    self.handle_hotkeys_command();
                    self.clear_editor();
                }
                SubmitAction::Fork => {
                    self.show_user_message_selector();
                    self.clear_editor();
                }
                SubmitAction::Tree => {
                    self.show_tree_selector();
                    self.clear_editor();
                }
                SubmitAction::Login => {
                    self.show_placeholder("oauth login selector");
                    self.clear_editor();
                }
                SubmitAction::Logout => {
                    self.show_placeholder("oauth logout selector");
                    self.clear_editor();
                }
                SubmitAction::New => {
                    self.clear_editor();
                    self.handle_new_session();
                }
                SubmitAction::Compact => {
                    let instructions = text
                        .strip_prefix("/compact")
                        .map(str::trim)
                        .filter(|s| !s.is_empty());
                    self.clear_editor();
                    self.handle_compact_command(instructions);
                }
                SubmitAction::Reload => {
                    self.clear_editor();
                    self.handle_reload_command();
                }
                SubmitAction::Debug => {
                    self.handle_debug_command();
                    self.clear_editor();
                }
                SubmitAction::ArminSaysHi => {
                    self.handle_armin_says_hi();
                    self.clear_editor();
                }
                SubmitAction::Resume => {
                    self.show_session_selector();
                    self.clear_editor();
                }
                SubmitAction::Quit => {
                    self.clear_editor();
                    self.shutdown();
                    return Ok(SubmitOutcome::Shutdown);
                }
                SubmitAction::Help => {
                    self.handle_help_command();
                    self.clear_editor();
                }
            }
            return Ok(SubmitOutcome::Ignored);
        }

        if text.starts_with('!') {
            let excluded = text.starts_with("!!");
            let command = if excluded {
                text[2..].trim()
            } else {
                text[1..].trim()
            };
            if !command.is_empty() {
                if self.is_bash_running {
                    self.show_warning(
                        "A bash command is already running. Press Esc to cancel it first.",
                    );
                    self.set_editor_text(&text);
                    return Ok(SubmitOutcome::Ignored);
                }
                self.push_editor_history(&text);
                self.handle_bash_command(command, excluded);
                self.set_bash_mode(false);
                self.clear_editor();
                return Ok(SubmitOutcome::Ignored);
            }
        }

        if self.is_compacting {
            if self.is_extension_command(&text) {
                self.push_editor_history(&text);
                self.clear_editor();
                self.chat_lines.push(format!("extension> {text}"));
            } else {
                self.queue_compaction_message(text, QueuedMessageKind::Steer);
            }
            return Ok(SubmitOutcome::Ignored);
        }

        if self.is_streaming {
            self.push_editor_history(&text);
            self.clear_editor();
            self.steering_queue.push_back(text);
            self.sync_pending_render_state();
            return Ok(SubmitOutcome::Ignored);
        }

        self.flush_pending_bash_components();
        if let Some(callback) = self.on_input_callback.as_mut() {
            callback(text.clone());
        }
        self.push_editor_history(&text);
        self.clear_editor();
        self.pending_working_message = Some(text);
        Ok(SubmitOutcome::Submitted)
    }

    async fn dispatch_prompt(&mut self, user_input: String) -> InteractiveResult<()> {
        self.controller
            .runtime_host
            .session_mut()
            .prompt(user_input.clone(), PromptOptions::default())
            .map_err(|err| -> Box<dyn Error + Send + Sync> { Box::new(err) })?;

        // Show user message IMMEDIATELY with background color (pi-style)
        self.render_state_mut()
            .add_message_to_chat(InteractiveMessage::User {
                text: user_input.clone(),
            });
        // Render now so user sees their message before streaming starts
        self.refresh_ui();

        // Reset streaming accumulators
        self.streaming_text.clear();
        self.streaming_thinking.clear();
        self.streaming_tool_calls.clear();
        self.is_streaming = true;

        // Append user message to session DB
        {
            let user_entry = bb_core::types::SessionEntry::Message {
                base: bb_core::types::EntryBase {
                    id: bb_core::types::EntryId::generate(),
                    parent_id: self.get_session_leaf(),
                    timestamp: chrono::Utc::now(),
                },
                message: bb_core::types::AgentMessage::User(bb_core::types::UserMessage {
                    content: vec![bb_core::types::ContentBlock::Text {
                        text: user_input.clone(),
                    }],
                    timestamp: chrono::Utc::now().timestamp_millis(),
                }),
            };
            store::append_entry(
                &self.session_setup.conn,
                &self.session_setup.session_id,
                &user_entry,
            ).map_err(|e| -> Box<dyn Error + Send + Sync> { Box::<dyn Error + Send + Sync>::from(e.to_string()) })?;
        }

        // Run the streaming turn loop
        self.run_streaming_turn_loop().await?;

        self.pending_working_message = None;
        self.rebuild_footer();
        self.refresh_ui();
        Ok(())
    }

    /// Drain steering queue first, then follow-up queue, dispatching each as a new prompt.
    async fn drain_queued_messages(&mut self) -> InteractiveResult<()> {
        // First drain all steering messages
        while let Some(text) = self.steering_queue.pop_front() {
            self.sync_pending_render_state();
            self.refresh_ui();
            self.dispatch_prompt(text).await?;
            if self.shutdown_requested {
                return Ok(());
            }
        }
        // Then drain all follow-up messages
        while let Some(text) = self.follow_up_queue.pop_front() {
            self.sync_pending_render_state();
            self.refresh_ui();
            self.dispatch_prompt(text).await?;
            if self.shutdown_requested {
                return Ok(());
            }
        }
        self.sync_pending_render_state();
        Ok(())
    }

    fn get_session_leaf(&self) -> Option<bb_core::types::EntryId> {
        store::get_session(&self.session_setup.conn, &self.session_setup.session_id)
            .ok()
            .flatten()
            .and_then(|s| s.leaf_id.map(bb_core::types::EntryId))
    }

    /// Run the full streaming turn loop: stream from provider, execute tools, loop until done.
    async fn run_streaming_turn_loop(&mut self) -> InteractiveResult<()> {
        let (tx, rx) = mpsc::unbounded_channel::<AgentLoopEvent>();
        self.agent_events = Some(rx);

        let mut turn_index: u32 = 0;

        loop {
            let _ = tx.send(AgentLoopEvent::TurnStart { turn_index });

            // Build context from session
            let ctx = bb_session::context::build_context(
                &self.session_setup.conn,
                &self.session_setup.session_id,
            ).map_err(|e| -> Box<dyn Error + Send + Sync> { Box::<dyn Error + Send + Sync>::from(e.to_string()) })?;

            let provider_messages = bb_core::agent_session::messages_to_provider(&ctx.messages);

            let request = bb_provider::CompletionRequest {
                system_prompt: self.session_setup.system_prompt.clone(),
                messages: provider_messages,
                tools: self.session_setup.tool_defs.clone(),
                model: self.session_setup.model.id.clone(),
                max_tokens: Some(self.session_setup.model.max_tokens as u32),
                stream: true,
                thinking: if self.session_setup.thinking_level == "off" { None } else { Some(self.session_setup.thinking_level.clone()) },
            };

            let options = bb_provider::RequestOptions {
                api_key: self.session_setup.api_key.clone(),
                base_url: self.session_setup.base_url.clone(),
                headers: std::collections::HashMap::new(),
                cancel: tokio_util::sync::CancellationToken::new(),
            };

            // Spawn provider streaming in background so tokens arrive while we render
            let (stream_tx, mut stream_rx) = mpsc::unbounded_channel();
            let provider_tx = tx.clone();
            let provider = &self.session_setup.provider;
            
            // We can't move provider into spawn, so run stream inline but 
            // forward events through the agent channel for the main loop
            let stream_result = provider.stream(request, options, stream_tx).await;

            if let Err(e) = stream_result {
                let raw = format!("{e}");
                let clean = raw.lines().next().unwrap_or(&raw).to_string();
                let _ = tx.send(AgentLoopEvent::Error { message: clean });
                let _ = tx.send(AgentLoopEvent::AssistantDone);
                break;
            }

            // Process stream events, forwarding as agent events with live rendering
            let mut all_events = Vec::new();
            while let Some(event) = stream_rx.recv().await {
                match &event {
                    bb_provider::StreamEvent::TextDelta { text } => {
                        // Update streaming text directly for immediate rendering
                        self.streaming_text.push_str(text);
                        self.update_streaming_display();
                    }
                    bb_provider::StreamEvent::ThinkingDelta { text } => {
                        self.streaming_thinking.push_str(text);
                        self.update_streaming_display();
                    }
                    bb_provider::StreamEvent::ToolCallStart { id, name } => {
                        let _ = tx.send(AgentLoopEvent::ToolCallStart { id: id.clone(), name: name.clone() });
                        self.drain_pending_agent_events();
                        self.refresh_ui();
                    }
                    bb_provider::StreamEvent::ToolCallDelta { id, arguments_delta } => {
                        let _ = tx.send(AgentLoopEvent::ToolCallDelta { id: id.clone(), args_delta: arguments_delta.clone() });
                    }
                    bb_provider::StreamEvent::Error { message } => {
                        let _ = tx.send(AgentLoopEvent::Error { message: message.clone() });
                        self.drain_pending_agent_events();
                        self.refresh_ui();
                    }
                    _ => {}
                }
                all_events.push(event);
            }
            // Final render after stream ends
            self.refresh_ui();

            let collected = bb_provider::streaming::CollectedResponse::from_events(&all_events);

            // Build assistant message and append to session
            let mut assistant_content = Vec::new();
            if !collected.thinking.is_empty() {
                assistant_content.push(bb_core::types::AssistantContent::Thinking { thinking: collected.thinking.clone() });
            }
            if !collected.text.is_empty() {
                assistant_content.push(bb_core::types::AssistantContent::Text { text: collected.text.clone() });
            }
            for tc in &collected.tool_calls {
                let args: serde_json::Value = serde_json::from_str(&tc.arguments).unwrap_or(serde_json::json!({}));
                assistant_content.push(bb_core::types::AssistantContent::ToolCall {
                    id: tc.id.clone(), name: tc.name.clone(), arguments: args,
                });
            }
            let assistant_msg = bb_core::types::AgentMessage::Assistant(bb_core::types::AssistantMessage {
                content: assistant_content,
                provider: self.session_setup.model.provider.clone(),
                model: self.session_setup.model.id.clone(),
                usage: bb_core::types::Usage {
                    input: collected.input_tokens,
                    output: collected.output_tokens,
                    ..Default::default()
                },
                stop_reason: if collected.tool_calls.is_empty() { bb_core::types::StopReason::Stop } else { bb_core::types::StopReason::ToolUse },
                error_message: None,
                timestamp: chrono::Utc::now().timestamp_millis(),
            });
            let asst_entry = bb_core::types::SessionEntry::Message {
                base: bb_core::types::EntryBase {
                    id: bb_core::types::EntryId::generate(),
                    parent_id: self.get_session_leaf(),
                    timestamp: chrono::Utc::now(),
                },
                message: assistant_msg,
            };
            store::append_entry(&self.session_setup.conn, &self.session_setup.session_id, &asst_entry)
                .map_err(|e| -> Box<dyn Error + Send + Sync> { Box::<dyn Error + Send + Sync>::from(e.to_string()) })?;

            // If no tool calls, we're done
            if collected.tool_calls.is_empty() {
                let _ = tx.send(AgentLoopEvent::TurnEnd { turn_index });
                let _ = tx.send(AgentLoopEvent::AssistantDone);
                self.drain_pending_agent_events();
                break;
            }

            // Execute tool calls
            let cancel = tokio_util::sync::CancellationToken::new();
            for tc in &collected.tool_calls {
                let args: serde_json::Value = serde_json::from_str(&tc.arguments).unwrap_or(serde_json::json!({}));
                let _ = tx.send(AgentLoopEvent::ToolExecuting { id: tc.id.clone(), name: tc.name.clone() });
                self.drain_pending_agent_events();
                self.refresh_ui();

                let tool = self.session_setup.tools.iter().find(|t| t.name() == tc.name);
                let result = match tool {
                    Some(t) => t.execute(args, &self.session_setup.tool_ctx, cancel.clone()).await,
                    None => Err(bb_core::error::BbError::Tool(format!("Unknown tool: {}", tc.name))),
                };
                let (content, is_error) = match result {
                    Ok(r) => (r.content, r.is_error),
                    Err(e) => (vec![bb_core::types::ContentBlock::Text { text: format!("Error: {e}") }], true),
                };
                let content_text = content.iter().filter_map(|c| match c {
                    bb_core::types::ContentBlock::Text { text } => Some(text.as_str()),
                    _ => None,
                }).collect::<Vec<_>>().join("\n");

                let _ = tx.send(AgentLoopEvent::ToolResult {
                    id: tc.id.clone(), name: tc.name.clone(), content: content_text, is_error,
                });
                self.drain_pending_agent_events();
                self.refresh_ui();

                // Append tool result to session
                let tool_result_entry = bb_core::types::SessionEntry::Message {
                    base: bb_core::types::EntryBase {
                        id: bb_core::types::EntryId::generate(),
                        parent_id: self.get_session_leaf(),
                        timestamp: chrono::Utc::now(),
                    },
                    message: bb_core::types::AgentMessage::ToolResult(bb_core::types::ToolResultMessage {
                        tool_call_id: tc.id.clone(),
                        tool_name: tc.name.clone(),
                        content,
                        details: None,
                        is_error,
                        timestamp: chrono::Utc::now().timestamp_millis(),
                    }),
                };
                store::append_entry(&self.session_setup.conn, &self.session_setup.session_id, &tool_result_entry)
                    .map_err(|e| -> Box<dyn Error + Send + Sync> { Box::<dyn Error + Send + Sync>::from(e.to_string()) })?;
            }

            let _ = tx.send(AgentLoopEvent::TurnEnd { turn_index });
            self.drain_pending_agent_events();
            turn_index += 1;
        }

        self.is_streaming = false;
        Ok(())
    }

    /// Update the streaming assistant display directly (bypasses event channel for lower latency)
    fn update_streaming_display(&mut self) {
        let message = assistant_message_from_parts(
            &self.streaming_text,
            if self.streaming_thinking.is_empty() { None } else { Some(self.streaming_thinking.clone()) },
            false,
        );
        if self.render_state().streaming_component.is_none() {
            // First text — create the streaming component
            let hide = self.hide_thinking_block;
            let label = self.hidden_thinking_label.clone();
            let mut comp = components::assistant_message::AssistantMessageComponent::new(
                Some(message.clone()), hide,
            );
            comp.set_hidden_thinking_label(label);
            self.render_state_mut().streaming_component = Some(comp.clone());
            self.render_state_mut().streaming_message = Some(message);
            self.render_state_mut().chat_items.push(ChatItem::AssistantMessage(comp));
        } else {
            // Update existing streaming component
            if let Some(comp) = self.render_state_mut().streaming_component.as_mut() {
                comp.update_content(message.clone());
            }
            self.render_state_mut().streaming_message = Some(message);
            // Update the last AssistantMessage in chat_items
            let updated = self.render_state().streaming_component.clone();
            if let Some(updated) = updated {
                if let Some(item) = self.render_state_mut().chat_items.iter_mut().rev()
                    .find(|i| matches!(i, ChatItem::AssistantMessage(_)))
                {
                    *item = ChatItem::AssistantMessage(updated);
                }
            }
        }
        self.refresh_ui();
    }

    /// Drain any pending agent events from the channel and handle them.
    fn drain_pending_agent_events(&mut self) {
        let mut events = Vec::new();
        if let Some(rx) = self.agent_events.as_mut() {
            while let Ok(event) = rx.try_recv() {
                events.push(event);
            }
        }
        for event in events {
            self.handle_agent_event(event);
        }
    }

    /// Drive the event loop while streaming is active, handling both
    /// terminal events (keyboard input) and agent events (streaming text,
    /// tool calls, done signals).
    async fn process_agent_events_until_done(&mut self) -> InteractiveResult<()> {
        while self.is_streaming {
            let terminal_events = self.events.as_mut();
            let agent_events = self.agent_events.as_mut();

            tokio::select! {
                // Terminal input events
                Some(event) = async {
                    match terminal_events {
                        Some(rx) => rx.recv().await,
                        None => std::future::pending().await,
                    }
                } => {
                    match event {
                        TerminalEvent::Resize(_, _) => {
                            self.ui.force_render();
                        }
                        TerminalEvent::Paste(data) | TerminalEvent::Raw(data) => {
                            self.ui.handle_raw_input(&data);
                            self.sync_bash_mode_from_editor();
                            self.refresh_ui();
                        }
                        TerminalEvent::Key(key) => {
                            // During streaming, Ctrl-C can interrupt
                            if key.code == KeyCode::Char('c')
                                && key.modifiers.contains(KeyModifiers::CONTROL)
                            {
                                self.handle_ctrl_c();
                                if self.shutdown_requested {
                                    self.is_streaming = false;
                                    break;
                                }
                            }
                            // Queue text input for after streaming
                            if key.code == KeyCode::Enter
                                && !key.modifiers.contains(KeyModifiers::SHIFT)
                            {
                                let text = self.editor_text();
                                let text = text.trim().to_string();
                                if !text.is_empty() {
                                    self.push_editor_history(&text);
                                    self.clear_editor();
                                    self.steering_queue.push_back(text);
                                    self.sync_pending_render_state();
                                    self.refresh_ui();
                                }
                            } else if key.code == KeyCode::F(9)
                                || (key.code == KeyCode::Enter
                                    && key.modifiers.contains(KeyModifiers::ALT))
                            {
                                let text = self.editor_text();
                                let text = text.trim().to_string();
                                if !text.is_empty() {
                                    self.push_editor_history(&text);
                                    self.clear_editor();
                                    self.follow_up_queue.push_back(text);
                                    self.sync_pending_render_state();
                                    self.refresh_ui();
                                }
                            } else if key.code == KeyCode::F(10) {
                                self.handle_dequeue();
                                self.refresh_ui();
                            } else {
                                self.ui.handle_key(&key);
                                self.sync_bash_mode_from_editor();
                                self.refresh_ui();
                            }
                        }
                    }
                },
                // Agent streaming events
                Some(agent_event) = async {
                    match agent_events {
                        Some(rx) => rx.recv().await,
                        None => std::future::pending().await,
                    }
                } => {
                    self.handle_agent_event(agent_event);
                    self.refresh_ui();
                },
                // Both channels closed
                else => {
                    self.is_streaming = false;
                    break;
                }
            }
        }

        // Clean up agent events channel
        self.agent_events = None;
        Ok(())
    }

    fn take_last_submitted_text(&mut self) -> String {
        self.pending_working_message
            .take()
            .unwrap_or_else(|| String::new())
    }

    fn sync_static_sections(&mut self) {
        self.rebuild_chat_container();
        self.rebuild_pending_container();
        self.rebuild_status_container();
        self.rebuild_footer();
    }

    fn refresh_ui(&mut self) {
        self.rebuild_chat_container();
        self.rebuild_pending_container();
        self.rebuild_status_container();
        self.rebuild_footer();
        // Always use differential render — never clear scrollback
        self.ui.render();
    }

    fn rebuild_header(&mut self) {
        self.header_lines.clear();
        if !self.options.quiet_startup {
            let dim = "\x1b[90m";
            let reset = "\x1b[0m";
            let bold = "\x1b[1m";
            let cyan = "\x1b[36m";
            self.header_lines.push(format!(
                "{bold}{cyan}BB-Agent{reset} v{}",
                self.version
            ));
            self.header_lines.push(format!(
                "{dim}Ctrl-C exit . / commands . ! bash . F2 thinking . /help for more{reset}"
            ));
        }

        if let Ok(mut header) = self.header_container.lock() {
            header.clear();
            if !self.header_lines.is_empty() {
                header.add(Box::new(Text::new(&self.header_lines.join("\n"))));
                header.add(Box::new(Spacer::new(1)));
            }
        }
    }

    fn rebuild_chat_container(&mut self) {
        let lines = self.chat_render_lines();
        Self::replace_container_lines(&self.chat_container, &lines);
    }

    fn rebuild_pending_container(&mut self) {
        self.sync_pending_render_state();
        let lines = self.pending_render_lines();
        Self::replace_container_lines(&self.pending_messages_container, &lines);
    }

    fn rebuild_status_container(&mut self) {
        let recent = self
            .status_lines
            .iter()
            .rev()
            .take(3)
            .cloned()
            .collect::<Vec<_>>();
        let mut recent = recent;
        recent.reverse();
        Self::replace_container_lines(&self.status_container, &recent);
    }

    fn rebuild_footer(&mut self) {
        let core_model = self
            .controller
            .runtime_host
            .runtime()
            .model
            .as_ref()
            .map(|model| format!("{}/{}", model.provider, model.id))
            .unwrap_or_else(|| "none".to_string());
        let cwd = self
            .controller
            .runtime_host
            .cwd()
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(".");

        // Pi-style footer: left=tokens cost context%  right=(provider) model . thinking
        let home = std::env::var("HOME").unwrap_or_default();
        let cwd_full = self.controller.runtime_host.cwd().display().to_string();
        let cwd_display = if !home.is_empty() && cwd_full.starts_with(&home) {
            format!("~{}", &cwd_full[home.len()..])
        } else {
            cwd.to_string()
        };

        let model_name = &self.session_setup.model.id;
        let provider_name = &self.session_setup.model.provider;
        let thinking_display = if self.hide_thinking_block { "off" } else { "high" };

        // Right side: (provider) model . thinking
        let right_side = format!("({provider_name}) {model_name} \u{2022} {thinking_display}");

        // Left side: $cost (sub) context%/window (auto)
        let chat_count = self.render_state().chat_items.len() + self.chat_lines.len();
        let left_side = if chat_count > 0 {
            format!("$0.000 (sub) 0.0%/{}k (auto)", self.session_setup.model.context_window / 1000)
        } else {
            format!("$0.000 (sub) 0.0%/{}k (auto)", self.session_setup.model.context_window / 1000)
        };

        // Build padded line
        let dim = "\x1b[90m";
        let reset = "\x1b[0m";
        let total_content = left_side.len() + right_side.len() + 2;
        let term_width = self.ui.columns() as usize;
        let padding = if term_width > total_content {
            " ".repeat(term_width - total_content)
        } else {
            "  ".to_string()
        };
        let footer = format!("{dim}{left_side}{padding}{right_side}{reset}");
        self.footer_lines = vec![footer];
        Self::replace_container_lines(&self.footer_container, &self.footer_lines);
    }

    fn render_widgets(&mut self) {
        // No extra spacing around editor — pi doesn't have it
        self.widgets_above_lines = vec![];
        self.widgets_below_lines = vec![];
        Self::replace_container_lines(&self.widget_container_above, &self.widgets_above_lines);
        Self::replace_container_lines(&self.widget_container_below, &self.widgets_below_lines);
    }

    fn replace_container_lines(container: &Arc<Mutex<Container>>, lines: &[String]) {
        if let Ok(mut container) = container.lock() {
            container.clear();
            if lines.is_empty() {
                return;
            }
            container.add(Box::new(Text::new(&lines.join("\n"))));
        }
    }

    fn editor_text(&self) -> String {
        self.editor
            .lock()
            .map(|e| e.get_text())
            .unwrap_or_default()
    }

    fn set_editor_text(&mut self, text: &str) {
        if let Ok(mut e) = self.editor.lock() {
            e.set_text(text);
        }
        self.sync_bash_mode_from_editor();
    }

    fn clear_editor(&mut self) {
        if let Ok(mut e) = self.editor.lock() {
            e.clear();
        }
        self.sync_bash_mode_from_editor();
    }

    fn push_editor_history(&mut self, text: &str) {
        if let Ok(mut e) = self.editor.lock() {
            e.add_to_history(text);
        }
    }

    fn set_bash_mode(&mut self, value: bool) {
        if let Ok(mut bash_mode) = self.is_bash_mode.lock() {
            *bash_mode = value;
        }
    }

    fn sync_bash_mode_from_editor(&mut self) {
        let is_bash_mode = self.editor_text().trim_start().starts_with('!');
        self.set_bash_mode(is_bash_mode);
    }

    fn start_background_checks(&mut self) {
        // Background checks are deferred - no TODO noise in the UI
    }

    fn get_changelog_for_display(&self) -> Option<String> {
        None
    }

    async fn bind_current_session_extensions(&mut self) -> InteractiveResult<()> {
        // Extension binding is deferred
        Ok(())
    }

    fn render_initial_messages(&mut self) {
        // No startup noise - pi doesn't show "initialized" messages
    }

    fn update_terminal_title(&mut self) {
        let cwd = self
            .controller
            .runtime_host
            .cwd()
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("BB-Agent");
        self.ui
            .terminal
            .write(&format!("\x1b]0;BB-Agent interactive - {cwd}\x07"));
    }

    fn stop_ui(&mut self) {
        self.ui.stop();
    }

    fn handle_escape(&mut self) {
        // Priority 1: dismiss overlay
        if self.ui.has_overlay() {
            self.ui.hide_overlay();
            return;
        }
        // Priority 2: abort loading animation
        if self.loading_animation {
            self.loading_animation = false;
            self.show_status("Aborted loading");
            return;
        }
        // Priority 3: cancel bash run
        if self.is_bash_running {
            self.is_bash_running = false;
            self.show_warning("Canceled bash placeholder run");
            return;
        }
        // Priority 4: exit bash mode
        if self
            .is_bash_mode
            .lock()
            .map(|value| *value)
            .unwrap_or(false)
        {
            self.clear_editor();
            self.set_bash_mode(false);
            self.show_status("Exited bash mode");
            return;
        }
        // Priority 5: abort streaming
        if self.is_streaming {
            self.is_streaming = false;
            self.show_warning("Aborted");
            return;
        }
        // Priority 6: clear editor if it has text
        if !self.editor_text().trim().is_empty() {
            self.clear_editor();
            self.show_status("Editor cleared");
            return;
        }
        // Priority 7: double-escape -> tree selector
        let now = Instant::now();
        let activate = self
            .last_escape_time
            .map(|last| now.saturating_duration_since(last) < Duration::from_millis(500))
            .unwrap_or(false);
        if activate {
            self.show_tree_selector();
            self.last_escape_time = None;
        } else {
            self.last_escape_time = Some(now);
        }
    }

    fn handle_ctrl_c(&mut self) {
        // If streaming, abort and show "Aborted"
        if self.is_streaming {
            self.is_streaming = false;
            self.show_warning("Aborted");
            self.last_sigint_time = Some(Instant::now());
            return;
        }
        // If editor has text, clear it
        if !self.editor_text().trim().is_empty() {
            self.clear_editor();
            self.show_status("Editor cleared");
            self.last_sigint_time = Some(Instant::now());
            return;
        }
        // Double Ctrl-C -> shutdown
        let now = Instant::now();
        let is_double = self
            .last_sigint_time
            .map(|last| now.saturating_duration_since(last) < Duration::from_millis(500))
            .unwrap_or(false);
        self.last_sigint_time = Some(now);

        if is_double {
            self.shutdown_requested = true;
            self.show_warning("Exiting interactive mode");
        } else {
            self.show_status("Interrupt requested. Press Ctrl-C again to exit.");
        }
    }

    fn handle_ctrl_d(&mut self) {
        if self.editor_text().trim().is_empty() {
            self.shutdown_requested = true;
            self.show_status("EOF received on empty editor; shutting down");
        }
    }

    fn handle_ctrl_z(&mut self) {
        self.show_warning("Suspend is not wired in the non-fullscreen skeleton yet");
    }

    fn cycle_thinking_level(&mut self) {
        let current = self.controller.runtime_host.session().thinking_level();
        let next = match current {
            ThinkingLevel::Off => ThinkingLevel::Low,
            ThinkingLevel::Low => ThinkingLevel::Medium,
            ThinkingLevel::Medium => ThinkingLevel::High,
            ThinkingLevel::High | ThinkingLevel::XHigh => ThinkingLevel::Off,
        };
        self.controller
            .runtime_host
            .session_mut()
            .set_thinking_level(next);
        let label = match next {
            ThinkingLevel::Off => "off",
            ThinkingLevel::Low => "low",
            ThinkingLevel::Medium => "medium",
            ThinkingLevel::High => "high",
            ThinkingLevel::XHigh => "xhigh",
        };
        self.show_status(format!("Thinking level: {label}"));
        self.rebuild_footer();
    }

    fn cycle_model(&mut self, direction: &str) {
        let mut models = self.get_model_candidates();
        if models.is_empty() {
            self.show_warning("No models available");
            return;
        }
        models.sort_by(|a, b| {
            a.provider
                .cmp(&b.provider)
                .then_with(|| a.id.cmp(&b.id))
        });

        let current_provider = self.session_setup.model.provider.clone();
        let current_id = self.session_setup.model.id.clone();
        let current_idx = models
            .iter()
            .position(|m| m.provider == current_provider && m.id == current_id)
            .unwrap_or(0);
        let next_idx = match direction {
            "backward" => {
                if current_idx == 0 { models.len() - 1 } else { current_idx - 1 }
            }
            _ => (current_idx + 1) % models.len(),
        };
        if let Some(model) = models.get(next_idx).cloned() {
            self.apply_model_selection(model);
        }
    }

    fn toggle_tool_output_expansion(&mut self) {
        self.tool_output_expanded = !self.tool_output_expanded;
        let state_label = if self.tool_output_expanded {
            "enabled"
        } else {
            "collapsed"
        };
        self.show_status(format!("tool output expansion {state_label}"));
        // Re-render chat to reflect new expansion state
        self.rebuild_chat_container();
        self.rebuild_pending_container();
    }

    fn toggle_thinking_block_visibility(&mut self) {
        self.hide_thinking_block = !self.hide_thinking_block;
        let state_label = if self.hide_thinking_block {
            "hidden"
        } else {
            "expanded"
        };
        self.show_status(format!("thinking block {state_label}"));
        // Re-render chat to reflect new thinking visibility
        self.rebuild_chat_container();
        self.rebuild_pending_container();
    }

    fn handle_follow_up(&mut self) {
        let text = self.editor_text().trim().to_string();
        if text.is_empty() {
            self.show_status("Nothing to queue as follow-up");
            return;
        }
        self.push_editor_history(&text);
        self.clear_editor();
        self.follow_up_queue.push_back(text);
        self.sync_pending_render_state();
        self.show_status("Queued follow-up message");
    }

    fn handle_dequeue(&mut self) {
        // Pop from follow-up queue first, then steering queue
        let popped = if let Some(text) = self.follow_up_queue.pop_back() {
            Some(text)
        } else {
            self.steering_queue.pop_back()
        };
        if let Some(text) = popped {
            let current = self.editor_text();
            if current.trim().is_empty() {
                self.set_editor_text(&text);
            } else {
                self.set_editor_text(&format!("{text}\n\n{current}"));
            }
            self.sync_pending_render_state();
            self.show_status("Restored queued message to editor");
        } else {
            self.show_status("No queued messages to restore");
        }
    }

    fn handle_clipboard_image_paste(&mut self) {
        self.show_status("TODO: clipboard image paste");
    }

    fn show_settings_selector(&mut self) {
        let _ = self.controller.commands.show_settings_selector();
        self.show_placeholder("settings selector");
    }

    fn handle_model_command(&mut self, search_term: Option<&str>) {
        let Some(search_term) = search_term.map(str::trim).filter(|s| !s.is_empty()) else {
            self.show_model_selector(None);
            return;
        };

        if let Some(model) = self.find_exact_model_match(search_term) {
            self.apply_model_selection(model);
            return;
        }

        self.show_model_selector(Some(search_term));
    }

    fn build_model_registry(&self) -> ModelRegistry {
        let mut registry = ModelRegistry::new();
        let settings = bb_core::settings::Settings::load_merged(&self.controller.runtime_host.cwd());
        registry.load_custom_models(&settings);
        registry
    }

    fn get_model_candidates(&self) -> Vec<Model> {
        self.build_model_registry().list().to_vec()
    }

    fn find_exact_model_match(&self, search_term: &str) -> Option<Model> {
        let needle = search_term.trim().to_ascii_lowercase();
        self.get_model_candidates().into_iter().find(|model| {
            let provider_id = format!("{}/{}", model.provider, model.id).to_ascii_lowercase();
            let provider_colon_id = format!("{}:{}", model.provider, model.id).to_ascii_lowercase();
            model.id.eq_ignore_ascii_case(&needle)
                || model.name.eq_ignore_ascii_case(&needle)
                || provider_id == needle
                || provider_colon_id == needle
        })
    }

    fn apply_model_selection(&mut self, model: Model) {
        let api_key = crate::login::resolve_api_key(&model.provider).unwrap_or_default();
        let base_url = model.base_url.clone().unwrap_or_else(|| match model.api {
            ApiType::AnthropicMessages => "https://api.anthropic.com".to_string(),
            ApiType::GoogleGenerative => "https://generativelanguage.googleapis.com".to_string(),
            _ => "https://api.openai.com/v1".to_string(),
        });
        let new_provider: Box<dyn bb_provider::Provider> = match model.api {
            ApiType::AnthropicMessages => Box::new(bb_provider::anthropic::AnthropicProvider::new()),
            ApiType::GoogleGenerative => Box::new(bb_provider::google::GoogleProvider::new()),
            _ => Box::new(bb_provider::openai::OpenAiProvider::new()),
        };
        let display = format!("{}/{}", model.provider, model.id);

        self.controller.runtime_host.session_mut().set_model(ModelRef {
            provider: model.provider.clone(),
            id: model.id.clone(),
            reasoning: model.reasoning,
        });
        self.controller.runtime_host.runtime_mut().model = Some(RuntimeModelRef {
            provider: model.provider.clone(),
            id: model.id.clone(),
            context_window: model.context_window as usize,
        });
        self.session_setup.model = model;
        self.session_setup.provider = new_provider;
        self.session_setup.api_key = api_key;
        self.session_setup.base_url = base_url;
        self.options.model_display = Some(display.clone());
        self.show_status(format!("Model: {display}"));
        self.rebuild_footer();
    }

    fn process_overlay_actions(&mut self) {
        let action = self
            .ui
            .topmost_overlay_as_mut::<ModelSelectorOverlay>()
            .and_then(|overlay| overlay.take_action());

        match action {
            Some(ModelSelectorOverlayAction::Selected(selection)) => {
                self.ui.hide_overlay();
                if let Some(model) = self
                    .get_model_candidates()
                    .into_iter()
                    .find(|m| m.provider == selection.provider && m.id == selection.model_id)
                {
                    self.apply_model_selection(model);
                } else {
                    self.show_warning(format!(
                        "Model not found: {}/{}",
                        selection.provider, selection.model_id
                    ));
                }
            }
            Some(ModelSelectorOverlayAction::Cancelled) => {
                self.ui.hide_overlay();
                self.show_status("Canceled model selector");
            }
            None => {}
        }
    }

    fn handle_export_command(&mut self, text: &str) {
        self.show_status(format!("TODO: export command {text}"));
    }

    fn handle_import_command(&mut self, text: &str) {
        self.show_status(format!("TODO: import command {text}"));
    }

    fn handle_share_command(&mut self) {
        self.show_status("TODO: share command");
    }

    fn handle_copy_command(&mut self) {
        self.show_status("TODO: copy command");
    }

    fn handle_name_command(&mut self, text: &str) {
        let name = text.strip_prefix("/name").unwrap_or(text).trim();
        if name.is_empty() {
            self.show_status("Usage: /name <session name>");
            return;
        }
        match self.session_setup.conn.execute(
            "UPDATE sessions SET name = ?1, updated_at = datetime('now') WHERE session_id = ?2",
            params![name, self.session_setup.session_id],
        ) {
            Ok(_) => self.show_status(format!("Session renamed to: {name}")),
            Err(e) => self.show_status(format!("Failed to rename session: {e}")),
        }
    }

    fn handle_session_command(&mut self) {
        let session_id = &self.session_setup.session_id;
        let model = self.options.model_display.as_deref().unwrap_or("unknown");
        let cwd = self.session_setup.tool_ctx.cwd.display().to_string();
        let msg_count = self.chat_lines.len() + self.render_state().chat_items.len();
        self.chat_lines.push(format!("Session ID:   {session_id}"));
        self.chat_lines.push(format!("Model:        {model}"));
        self.chat_lines.push(format!("Working dir:  {cwd}"));
        self.chat_lines.push(format!("Messages:     {msg_count}"));
    }

    fn handle_changelog_command(&mut self) {
        self.show_status("TODO: changelog command");
    }

    fn handle_hotkeys_command(&mut self) {
        let hotkeys = vec![
            "Key Bindings:",
            "  Ctrl+C      - Interrupt / clear input",
            "  Ctrl+D      - Exit (on empty input)",
            "  Ctrl+Z      - Suspend",
            "  Ctrl+J      - Cycle thinking level",
            "  Ctrl+K      - Cycle model forward",
            "  Ctrl+L      - Toggle tool output expansion",
            "  Ctrl+T      - Toggle thinking visibility",
            "  Ctrl+E      - Open external editor",
            "  Ctrl+R      - Resume session selector",
            "  Ctrl+N      - New session",
            "  Ctrl+F      - Follow-up message",
            "  Ctrl+V      - Paste image from clipboard",
            "  Esc         - Cancel / back",
        ];
        for line in hotkeys {
            self.chat_lines.push(line.to_string());
        }
    }

    fn show_user_message_selector(&mut self) {
        let _ = self.controller.commands.show_user_message_selector();
        self.show_placeholder("user message selector");
    }

    fn show_tree_selector(&mut self) {
        let _ = self.controller.commands.open_placeholder_selector(
            self::interactive_commands::SelectorKind::Tree,
            "Session Tree",
        );
        self.show_placeholder("session tree selector");
    }

    fn handle_clear_command(&mut self) {
        let _ = self.controller.runtime_host.session_mut().clear_queue();
        self.chat_lines.clear();
        self.pending_lines.clear();
        self.steering_queue.clear();
        self.follow_up_queue.clear();
        self.compaction_queued_messages.clear();
        self.render_state_mut().chat_items.clear();
        self.render_state_mut().pending_items.clear();
        self.show_status("Started a fresh interactive session shell around the core session");
    }

    fn handle_new_session(&mut self) {
        let cwd_str = self.session_setup.tool_ctx.cwd.display().to_string();
        match store::create_session(&self.session_setup.conn, &cwd_str) {
            Ok(new_id) => {
                self.session_setup.session_id = new_id.clone();
                self.options.session_id = Some(new_id.clone());
                let _ = self.controller.runtime_host.session_mut().clear_queue();
                self.chat_lines.clear();
                self.pending_lines.clear();
                self.compaction_queued_messages.clear();
                self.render_state_mut().chat_items.clear();
                self.render_state_mut().pending_items.clear();
                self.show_status(format!("New session created: {new_id}"));
            }
            Err(e) => {
                self.show_status(format!("Failed to create new session: {e}"));
            }
        }
    }

    fn handle_help_command(&mut self) {
        let commands = vec![
            "Available commands:",
            "  /help        - Show this help message",
            "  /new         - Create a new session",
            "  /name <name> - Rename current session",
            "  /session     - Show session info",
            "  /compact     - Trigger conversation compaction",
            "  /clear       - Clear chat display",
            "  /model       - Switch model",
            "  /hotkeys     - Show key bindings",
            "  /export      - Export session",
            "  /import      - Import session",
            "  /share       - Share session",
            "  /copy        - Copy last response",
            "  /debug       - Show debug info",
            "  /reload      - Reload resources",
            "  /quit        - Exit the application",
            "  !<cmd>       - Execute bash command",
            "  !!<cmd>      - Execute bash (excluded from context)",
        ];
        for line in commands {
            self.chat_lines.push(line.to_string());
        }
    }

    fn check_auto_compaction(&mut self) {
        let session_id = self.session_setup.session_id.clone();
        let settings = bb_core::types::CompactionSettings::default();
        if let Ok(entries) = store::get_entries(&self.session_setup.conn, &session_id) {
            let total_tokens: u64 = entries.iter().map(compaction::estimate_tokens_row).sum();
            let window = self.session_setup.model.context_window;
            if compaction::should_compact(total_tokens, window, &settings) {
                self.chat_lines.push(format!(
                    "[c] Auto-compaction triggered ({total_tokens} tokens, window {window})"
                ));
                // Prepare and note - full async LLM summarization deferred to future wave
                if let Some(prep) = compaction::prepare_compaction(&entries, &settings) {
                    self.chat_lines.push(format!(
                        "[c] {} messages to summarize, {} kept",
                        prep.messages_to_summarize.len(),
                        prep.kept_messages.len()
                    ));
                }
            }
        }
    }

    fn handle_compact_command(&mut self, instructions: Option<&str>) {
        self.is_compacting = true;
        let session_id = self.session_setup.session_id.clone();
        match store::get_entries(&self.session_setup.conn, &session_id) {
            Ok(entries) => {
                let settings = bb_core::types::CompactionSettings::default();
                let total_tokens: u64 = entries.iter().map(compaction::estimate_tokens_row).sum();
                match compaction::prepare_compaction(&entries, &settings) {
                    Some(prep) => {
                        let to_summarize = prep.messages_to_summarize.len();
                        let kept = prep.kept_messages.len();
                        self.chat_lines.push(format!(
                            "Compaction: {total_tokens} estimated tokens, {to_summarize} messages to summarize, {kept} kept"
                        ));
                        if let Some(inst) = instructions {
                            self.chat_lines.push(format!("Instructions: {inst}"));
                        }
                        self.show_status("Compaction prepared (async LLM summarization not wired in interactive mode yet)");
                    }
                    None => {
                        self.show_status(format!("Nothing to compact ({total_tokens} estimated tokens, {} entries)", entries.len()));
                    }
                }
            }
            Err(e) => {
                self.show_status(format!("Failed to get entries for compaction: {e}"));
            }
        }
        self.is_compacting = false;
    }

    fn handle_reload_command(&mut self) {
        self.show_status("TODO: reload resources/extensions");
    }

    fn handle_debug_command(&mut self) {
        self.show_status("TODO: debug command");
    }

    fn handle_armin_says_hi(&mut self) {
        self.render_state_mut()
            .add_message_to_chat(InteractiveMessage::Assistant {
                message: assistant_message_from_parts("hi armin 👋", None, false),
                tool_calls: Vec::new(),
            });
    }

    fn show_session_selector(&mut self) {
        let _ = self.controller.commands.open_placeholder_selector(
            self::interactive_commands::SelectorKind::Session,
            "Session Selector",
        );
        self.show_placeholder("session selector");
    }

    fn shutdown(&mut self) {
        self.shutdown_requested = true;
        self.show_status("Shutdown requested");
    }

    fn handle_bash_command(&mut self, command: &str, excluded_from_context: bool) {
        let label = if excluded_from_context { "bash(excluded)" } else { "bash" };
        self.chat_lines.push(format!("{label}> {command}"));
        let output = std::process::Command::new("sh")
            .arg("-c")
            .arg(command)
            .current_dir(&self.session_setup.tool_ctx.cwd)
            .output();
        match output {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                let stderr = String::from_utf8_lossy(&out.stderr);
                if !stdout.is_empty() {
                    for line in stdout.lines() {
                        self.chat_lines.push(line.to_string());
                    }
                }
                if !stderr.is_empty() {
                    for line in stderr.lines() {
                        self.chat_lines.push(format!("stderr: {line}"));
                    }
                }
                if !out.status.success() {
                    self.chat_lines.push(format!("exit code: {}", out.status.code().unwrap_or(-1)));
                }
            }
            Err(e) => {
                self.chat_lines.push(format!("Failed to execute command: {e}"));
            }
        }
    }

    fn flush_pending_bash_components(&mut self) {
        while let Some(line) = self.pending_bash_components.pop_front() {
            self.chat_lines.push(line);
        }
    }

    fn is_extension_command(&self, text: &str) -> bool {
        text.starts_with("/ext") || text.starts_with("/extension")
    }

    fn queue_compaction_message(&mut self, text: String, kind: QueuedMessageKind) {
        self.compaction_queued_messages
            .push_back(QueuedMessage { text, kind });
        self.show_status("Queued message while compaction is active");
    }

    fn show_model_selector(&mut self, initial_search: Option<&str>) {
        let current_model = self
            .controller
            .runtime_host
            .session()
            .model()
            .map(|m| format!("{}/{}", m.provider, m.id))
            .unwrap_or_else(|| format!("{}/{}", self.session_setup.model.provider, self.session_setup.model.id));

        let mut models = self.get_model_candidates();
        let current_provider = self.session_setup.model.provider.clone();
        let current_id = self.session_setup.model.id.clone();
        models.sort_by(|a, b| {
            let a_current = a.provider == current_provider && a.id == current_id;
            let b_current = b.provider == current_provider && b.id == current_id;
            b_current
                .cmp(&a_current)
                .then_with(|| a.provider.cmp(&b.provider))
                .then_with(|| a.id.cmp(&b.id))
        });

        let mut selector = ModelSelector::from_models(models, 10);
        if let Some(query) = initial_search.filter(|s| !s.is_empty()) {
            selector.set_search(query);
        }
        let component = Box::new(ModelSelectorOverlay::new(
            selector,
            current_model,
            initial_search.map(|s| s.to_string()),
        ));
        self.ui.show_overlay(component);
        self.show_status("Opened model selector");
    }

    fn show_placeholder(&mut self, label: &str) {
        self.show_status(format!("TODO: {label}"));
    }

    fn handle_agent_event(&mut self, event: AgentLoopEvent) {
        match event {
            AgentLoopEvent::TurnStart { .. } => {
                // Reset streaming state for a new turn
                self.streaming_text.clear();
                self.streaming_thinking.clear();
            }
            AgentLoopEvent::TextDelta { text } => {
                self.streaming_text.push_str(&text);
                if self.render_state().streaming_component.is_none() {
                    // Create a new streaming assistant message
                    self.is_streaming = true;
                    self.pending_working_message = None;
                    let message = assistant_message_from_parts(
                        &self.streaming_text,
                        if self.streaming_thinking.is_empty() {
                            None
                        } else {
                            Some(self.streaming_thinking.clone())
                        },
                        false,
                    );
                    let hide_thinking = self.hide_thinking_block;
                    let label = self.hidden_thinking_label.clone();
                    let mut component = components::assistant_message::AssistantMessageComponent::new(
                        Some(message.clone()),
                        hide_thinking,
                    );
                    component.set_hidden_thinking_label(label);
                    self.render_state_mut().streaming_component = Some(component.clone());
                    self.render_state_mut().streaming_message = Some(message);
                    self.render_state_mut().chat_items.push(ChatItem::AssistantMessage(component));
                } else {
                    // Update the existing streaming assistant message
                    let message = assistant_message_from_parts(
                        &self.streaming_text,
                        if self.streaming_thinking.is_empty() {
                            None
                        } else {
                            Some(self.streaming_thinking.clone())
                        },
                        false,
                    );
                    if let Some(component) = self.render_state_mut().streaming_component.as_mut() {
                        component.update_content(message.clone());
                    }
                    self.render_state_mut().streaming_message = Some(message.clone());
                    // Update the last AssistantMessage chat item
                    let updated_component = self.render_state().streaming_component.clone();
                    if let Some(updated_component) = updated_component {
                        if let Some(chat_item) = self.render_state_mut().chat_items.iter_mut().rev()
                            .find(|item| matches!(item, ChatItem::AssistantMessage(_)))
                        {
                            *chat_item = ChatItem::AssistantMessage(updated_component);
                        }
                    }
                }
                self.refresh_ui();
            }
            AgentLoopEvent::ThinkingDelta { text } => {
                self.streaming_thinking.push_str(&text);
                // Update streaming component with new thinking content
                if self.render_state().streaming_component.is_some() {
                    let message = assistant_message_from_parts(
                        &self.streaming_text,
                        Some(self.streaming_thinking.clone()),
                        false,
                    );
                    if let Some(component) = self.render_state_mut().streaming_component.as_mut() {
                        component.update_content(message.clone());
                    }
                    self.render_state_mut().streaming_message = Some(message.clone());
                    let updated_component = self.render_state().streaming_component.clone();
                    if let Some(updated_component) = updated_component {
                        if let Some(chat_item) = self.render_state_mut().chat_items.iter_mut().rev()
                            .find(|item| matches!(item, ChatItem::AssistantMessage(_)))
                        {
                            *chat_item = ChatItem::AssistantMessage(updated_component);
                        }
                    }
                }
                self.refresh_ui();
            }
            AgentLoopEvent::ToolCallStart { id, name } => {
                let args = serde_json::Value::Null;
                let mut component = components::tool_execution::ToolExecutionComponent::new(
                    name,
                    id.clone(),
                    args,
                    components::tool_execution::ToolExecutionOptions {
                        show_images: self.render_state().show_images,
                    },
                );
                component.set_expanded(self.tool_output_expanded);
                self.render_state_mut().chat_items.push(ChatItem::ToolExecution(component.clone()));
                self.render_state_mut().pending_tools.insert(id, component);
                self.refresh_ui();
            }
            AgentLoopEvent::ToolCallDelta { id, args_delta } => {
                if let Some(component) = self.render_state_mut().pending_tools.get_mut(&id) {
                    // Append delta to existing args. For simplicity, merge as string into args.
                    let current = component.args().clone();
                    let new_args = match current {
                        serde_json::Value::String(s) => {
                            serde_json::Value::String(format!("{s}{args_delta}"))
                        }
                        serde_json::Value::Null => {
                            serde_json::Value::String(args_delta)
                        }
                        other => other,
                    };
                    component.update_args(new_args);
                }
                // Update the corresponding chat item
                let updated = self.render_state().pending_tools.get(&id).cloned();
                if let Some(updated) = updated {
                    let id_clone = id.clone();
                    if let Some(chat_item) = self.render_state_mut().chat_items.iter_mut().rev()
                        .find(|item| matches!(item, ChatItem::ToolExecution(tc) if tc.tool_call_id() == id_clone))
                    {
                        *chat_item = ChatItem::ToolExecution(updated);
                    }
                }
                self.refresh_ui();
            }
            AgentLoopEvent::ToolExecuting { id, .. } => {
                let updated = {
                    if let Some(component) = self.render_state_mut().pending_tools.get_mut(&id) {
                        component.mark_execution_started();
                        Some(component.clone())
                    } else {
                        None
                    }
                };
                if let Some(updated) = updated {
                    let id_clone = id.clone();
                    if let Some(chat_item) = self.render_state_mut().chat_items.iter_mut().rev()
                        .find(|item| matches!(item, ChatItem::ToolExecution(tc) if tc.tool_call_id() == id_clone))
                    {
                        *chat_item = ChatItem::ToolExecution(updated);
                    }
                }
                self.refresh_ui();
            }
            AgentLoopEvent::ToolResult { id, name: _, content, is_error } => {
                let updated = {
                    if let Some(component) = self.render_state_mut().pending_tools.get_mut(&id) {
                        let result = components::tool_execution::ToolExecutionResult {
                            content: vec![components::tool_execution::ToolResultBlock {
                                r#type: "text".to_string(),
                                text: Some(content),
                                data: None,
                                mime_type: None,
                            }],
                            is_error,
                            details: None,
                        };
                        component.update_result(result, false);
                        Some(component.clone())
                    } else {
                        None
                    }
                };
                if let Some(updated) = updated {
                    let id_clone = id.clone();
                    if let Some(chat_item) = self.render_state_mut().chat_items.iter_mut().rev()
                        .find(|item| matches!(item, ChatItem::ToolExecution(tc) if tc.tool_call_id() == id_clone))
                    {
                        *chat_item = ChatItem::ToolExecution(updated);
                    }
                }
                self.render_state_mut().pending_tools.remove(&id);
                self.refresh_ui();
            }
            AgentLoopEvent::TurnEnd { .. } => {
                // no-op
            }
            AgentLoopEvent::AssistantDone => {
                self.is_streaming = false;
                self.streaming_text.clear();
                self.streaming_thinking.clear();
                self.pending_working_message = None;
                self.render_state_mut().streaming_component = None;
                self.render_state_mut().streaming_message = None;
                self.render_state_mut().pending_tools.clear();
                // Auto-compaction check
                self.check_auto_compaction();
                // Dispatch queued steering messages
                if let Some(queued) = self.steering_queue.pop_front() {
                    self.chat_lines.push(format!("queued(steer)> {queued}"));
                    self.pending_working_message = Some(queued);
                }
                self.rebuild_footer();
                self.refresh_ui();
            }
            AgentLoopEvent::Error { message } => {
                self.is_streaming = false;
                self.streaming_text.clear();
                self.streaming_thinking.clear();
                self.render_state_mut().streaming_component = None;
                self.render_state_mut().streaming_message = None;
                self.render_state_mut().pending_tools.clear();
                self.render_state_mut().add_message_to_chat(InteractiveMessage::Custom {
                    custom_type: "error".to_string(),
                    text: format!("Error: {message}"),
                    display: true,
                });
                self.rebuild_footer();
                self.refresh_ui();
            }
        }
    }

    fn show_warning(&mut self, message: impl Into<String>) {
        let message = message.into();
        let dim = "\x1b[90m";
        let yellow = "\x1b[33m";
        let reset = "\x1b[0m";
        self.render_state_mut().last_status = Some(format!("{yellow}[!]{reset} {dim}{message}{reset}"));
        // Only keep latest status visible
        self.status_lines = vec![format!("{yellow}[!]{reset} {dim}{message}{reset}")];
    }

    fn show_status(&mut self, message: impl Into<String>) {
        let message = message.into();
        let dim = "\x1b[90m";
        let reset = "\x1b[0m";
        self.render_state_mut().last_status = Some(format!("{dim}{message}{reset}"));
        self.status_lines = vec![format!("{dim}{message}{reset}")];
    }
}

pub async fn run_interactive(
    runtime_host: AgentSessionRuntimeHost,
    options: InteractiveModeOptions,
    session_setup: InteractiveSessionSetup,
) -> InteractiveResult<()> {
    let mut mode = InteractiveMode::new(runtime_host, options, session_setup);
    mode.run().await
}

struct SharedEditorWrapper {
    inner: Arc<Mutex<Editor>>,
}

impl SharedEditorWrapper {
    fn new(inner: Arc<Mutex<Editor>>) -> Self {
        Self { inner }
    }
}

impl Component for SharedEditorWrapper {
    fn render(&self, width: u16) -> Vec<String> {
        self.inner
            .lock()
            .map(|inner| inner.render(width))
            .unwrap_or_else(|_| vec!["<editor unavailable>".to_string()])
    }

    fn handle_input(&mut self, key: &KeyEvent) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.handle_input(key);
        }
    }

    fn handle_raw_input(&mut self, data: &str) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.handle_raw_input(data);
        }
    }

    fn invalidate(&mut self) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.invalidate();
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

pub use interactive_commands::*;
