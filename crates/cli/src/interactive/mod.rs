#![allow(dead_code)]

pub mod components;
#[path = "../interactive_events.rs"]
pub mod events;
#[path = "../interactive_commands.rs"]
pub mod interactive_commands;

use self::events::{
    ChatItem, InteractiveMessage, InteractiveRenderState, PendingMessages,
    QueuedMessage as RenderQueuedMessage, QueuedMessageMode, assistant_message_from_parts,
};
use self::interactive_commands::InteractiveCommands;
use bb_core::agent_session::PromptOptions;
use bb_core::agent_session_runtime::AgentSessionRuntimeHost;
use bb_tui::component::{CURSOR_MARKER, Component, Container, Spacer, Text};
use bb_tui::terminal::{Terminal, TerminalEvent};
use bb_tui::tui_core::TUI;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::any::Any;
use std::collections::VecDeque;
use std::error::Error;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
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

#[derive(Debug, Default)]
struct InteractiveSessionState {
    render_state: InteractiveRenderState,
    pending_messages: PendingMessages,
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

#[derive(Debug, Default)]
struct EditorState {
    text: String,
    cursor: usize,
    focused: bool,
    history: Vec<String>,
}

impl EditorState {
    fn set_text(&mut self, text: impl Into<String>) {
        self.text = text.into();
        self.cursor = self.text.len();
    }

    fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
    }

    fn text(&self) -> String {
        self.text.clone()
    }

    fn push_history(&mut self, text: impl Into<String>) {
        self.history.push(text.into());
    }

    fn insert_char(&mut self, ch: char) {
        if self.cursor >= self.text.len() {
            self.text.push(ch);
        } else {
            self.text.insert(self.cursor, ch);
        }
        self.cursor += ch.len_utf8();
    }

    fn insert_str(&mut self, s: &str) {
        if self.cursor >= self.text.len() {
            self.text.push_str(s);
        } else {
            self.text.insert_str(self.cursor, s);
        }
        self.cursor += s.len();
    }

    fn backspace(&mut self) {
        if self.cursor == 0 || self.text.is_empty() {
            return;
        }
        if let Some((idx, _)) = self.text[..self.cursor].char_indices().last() {
            self.text.drain(idx..self.cursor);
            self.cursor = idx;
        }
    }

    fn move_left(&mut self) {
        if self.cursor == 0 {
            return;
        }
        if let Some((idx, _)) = self.text[..self.cursor].char_indices().last() {
            self.cursor = idx;
        } else {
            self.cursor = 0;
        }
    }

    fn move_right(&mut self) {
        if self.cursor >= self.text.len() {
            return;
        }
        let next = self.text[self.cursor..]
            .char_indices()
            .nth(1)
            .map(|(idx, _)| self.cursor + idx)
            .unwrap_or(self.text.len());
        self.cursor = next;
    }
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

#[derive(Debug)]
struct EditorComponent {
    state: Arc<Mutex<EditorState>>,
    bash_mode: Arc<Mutex<bool>>,
}

impl EditorComponent {
    fn new(state: Arc<Mutex<EditorState>>, bash_mode: Arc<Mutex<bool>>) -> Self {
        Self { state, bash_mode }
    }
}

impl Component for EditorComponent {
    fn render(&self, _width: u16) -> Vec<String> {
        let state = self.state.lock();
        let bash_mode = self.bash_mode.lock();
        match (state, bash_mode) {
            (Ok(state), Ok(bash_mode)) => {
                let prompt = if *bash_mode { "!" } else { ">" };
                let mut line = format!("{prompt} {}", state.text);
                if state.focused {
                    line.push_str(CURSOR_MARKER);
                }
                vec![line]
            }
            _ => vec!["<editor unavailable>".to_string()],
        }
    }

    fn handle_input(&mut self, key: &KeyEvent) {
        if let Ok(mut state) = self.state.lock() {
            match key.code {
                KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    state.insert_char(ch)
                }
                KeyCode::Backspace => state.backspace(),
                KeyCode::Left => state.move_left(),
                KeyCode::Right => state.move_right(),
                KeyCode::Home => state.cursor = 0,
                KeyCode::End => state.cursor = state.text.len(),
                _ => {}
            }
        }
    }

    fn handle_raw_input(&mut self, data: &str) {
        if let Ok(mut state) = self.state.lock() {
            state.insert_str(data);
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
    ui: TUI,
    header_container: Arc<Mutex<Container>>,
    chat_container: Arc<Mutex<Container>>,
    pending_messages_container: Arc<Mutex<Container>>,
    status_container: Arc<Mutex<Container>>,
    widget_container_above: Arc<Mutex<Container>>,
    widget_container_below: Arc<Mutex<Container>>,
    footer_container: Arc<Mutex<Container>>,
    editor_component: Arc<Mutex<EditorComponent>>,
    editor_state: Arc<Mutex<EditorState>>,
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
    pending_bash_components: VecDeque<String>,
    compaction_queued_messages: VecDeque<QueuedMessage>,
    key_handlers: Vec<(KeyBinding, KeyAction)>,
    submit_routes: Vec<SubmitRoute>,
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
    pub fn new(runtime_host: AgentSessionRuntimeHost, options: InteractiveModeOptions) -> Self {
        let editor_state = Arc::new(Mutex::new(EditorState {
            focused: true,
            ..EditorState::default()
        }));
        let is_bash_mode = Arc::new(Mutex::new(false));
        let editor_component = Arc::new(Mutex::new(EditorComponent::new(
            editor_state.clone(),
            is_bash_mode.clone(),
        )));

        let mut this = Self {
            controller: InteractiveController::new(runtime_host),
            ui: TUI::new(),
            header_container: Arc::new(Mutex::new(Container::new())),
            chat_container: Arc::new(Mutex::new(Container::new())),
            pending_messages_container: Arc::new(Mutex::new(Container::new())),
            status_container: Arc::new(Mutex::new(Container::new())),
            widget_container_above: Arc::new(Mutex::new(Container::new())),
            widget_container_below: Arc::new(Mutex::new(Container::new())),
            footer_container: Arc::new(Mutex::new(Container::new())),
            editor_component,
            editor_state,
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
            pending_bash_components: VecDeque::new(),
            compaction_queued_messages: VecDeque::new(),
            key_handlers: Vec::new(),
            submit_routes: Vec::new(),
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
                },
            })
            .collect::<Vec<_>>();
        let pending = InteractiveRenderState::collect_pending_messages(&[], &[], &queued);
        self.controller.session.pending_messages = pending.clone();
        self.render_state_mut()
            .update_pending_messages_display(&pending);
    }

    fn render_items_to_lines(items: &[ChatItem]) -> Vec<String> {
        items
            .iter()
            .flat_map(|item| match item {
                ChatItem::Spacer => vec![String::new()],
                ChatItem::UserMessage(text) => vec![format!("you> {text}")],
                ChatItem::AssistantMessage(component) => component.render_lines(),
                ChatItem::ToolExecution(component) => component.render_lines(),
                ChatItem::BashExecution(component) => component.render_lines(),
                ChatItem::CustomMessage { text, .. } => vec![text.clone()],
                ChatItem::CompactionSummary(summary) => vec![format!("compaction> {summary}")],
                ChatItem::BranchSummary(summary) => vec![format!("branch> {summary}")],
                ChatItem::PendingMessageLine(line) => vec![line.clone()],
            })
            .collect()
    }

    fn chat_render_lines(&self) -> Vec<String> {
        let mut lines = Self::render_items_to_lines(&self.render_state().chat_items);
        lines.extend(self.chat_lines.iter().cloned());
        lines
    }

    fn pending_render_lines(&self) -> Vec<String> {
        let mut lines = Self::render_items_to_lines(&self.render_state().pending_items);
        lines.extend(self.pending_lines.iter().cloned());
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
            .add(Box::new(SharedEditor::new(self.editor_component.clone())));
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
        }

        for message in self.options.initial_messages.clone() {
            self.dispatch_prompt(message).await?;
        }

        while !self.shutdown_requested {
            let Some(user_input) = self.get_user_input().await? else {
                break;
            };
            self.dispatch_prompt(user_input).await?;
        }

        self.stop_ui();
        Ok(())
    }

    async fn get_user_input(&mut self) -> InteractiveResult<Option<String>> {
        loop {
            if self.shutdown_requested {
                return Ok(None);
            }

            let event = match self.events.as_mut() {
                Some(events) => events.recv().await,
                None => None,
            };

            let Some(event) = event else {
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
    }

    async fn handle_key_event(&mut self, key: KeyEvent) -> InteractiveResult<Option<String>> {
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
            KeyAction::SelectModel => self.show_placeholder("model selector"),
            KeyAction::ToggleToolExpansion => self.toggle_tool_output_expansion(),
            KeyAction::ToggleThinkingVisibility => self.toggle_thinking_block_visibility(),
            KeyAction::OpenExternalEditor => self.show_placeholder("external editor"),
            KeyAction::FollowUp => self.handle_follow_up(),
            KeyAction::Dequeue => self.handle_dequeue(),
            KeyAction::SessionNew => self.handle_clear_command(),
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
                    self.handle_clear_command();
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
            self.pending_lines.push(format!("queued> {text}"));
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

        self.render_state_mut()
            .add_message_to_chat(InteractiveMessage::User {
                text: user_input.clone(),
            });
        self.render_state_mut().add_message_to_chat(InteractiveMessage::Assistant {
            message: assistant_message_from_parts(
                "TODO: core session prompt dispatch is bootstrapped; event wiring still pending",
                None,
                false,
            ),
            tool_calls: Vec::new(),
        });
        self.pending_working_message = None;
        self.rebuild_footer();
        self.refresh_ui();
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
        self.ui.render();
    }

    fn rebuild_header(&mut self) {
        self.header_lines.clear();
        if self.options.verbose || !self.options.quiet_startup {
            self.header_lines
                .push(format!("BB-Agent v{}", self.version));
            if let Some(model_display) = &self.options.model_display {
                self.header_lines
                    .push(format!("Model: {}", model_display));
            }
            if let Some(session_id) = &self.options.session_id {
                self.header_lines
                    .push(format!("Session: {}", &session_id[..8.min(session_id.len())]));
            }
            if self.options.agents_md.is_some() {
                self.header_lines
                    .push("AGENTS.md loaded".to_string());
            }
            self.header_lines.push(
                "Ctrl-C interrupt/exit • Ctrl-D exit(empty) • Esc clears bash mode".to_string(),
            );
            self.header_lines.push("F2 thinking • F3/F4 model • F6 tools • F7 thinking block • / for commands • ! for bash".to_string());
            if let Some(changelog) = self.changelog_markdown.clone() {
                self.header_lines.push(String::new());
                self.header_lines.push("What’s New".to_string());
                self.header_lines
                    .extend(changelog.lines().map(ToOwned::to_owned));
            }
        } else if let Some(changelog) = self.changelog_markdown.clone() {
            self.header_lines.push(format!(
                "Updated recently. Use /changelog for details. {}",
                changelog.lines().next().unwrap_or_default()
            ));
        }

        if let Ok(mut header) = self.header_container.lock() {
            header.clear();
            header.add(Box::new(Spacer::new(1)));
            if !self.header_lines.is_empty() {
                header.add(Box::new(Text::new(&self.header_lines.join("\n"))));
            }
            header.add(Box::new(Spacer::new(1)));
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

        self.footer_lines = vec![format!(
            "state: init={} stream={} compact={} bash={} queued(ui/core)={}/{} chat={} model={} cwd={}",
            self.is_initialized,
            self.is_streaming,
            self.is_compacting,
            self.is_bash_mode.lock().map(|v| *v).unwrap_or(false),
            self.controller.session.pending_messages.combined().len(),
            self.controller
                .runtime_host
                .session()
                .pending_message_count(),
            self.render_state().chat_items.len() + self.chat_lines.len(),
            core_model,
            cwd,
        )];
        Self::replace_container_lines(&self.footer_container, &self.footer_lines);
    }

    fn render_widgets(&mut self) {
        self.widgets_above_lines = vec![String::new()];
        self.widgets_below_lines = vec![String::new()];
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
        self.editor_state
            .lock()
            .map(|state| state.text())
            .unwrap_or_default()
    }

    fn set_editor_text(&mut self, text: &str) {
        if let Ok(mut state) = self.editor_state.lock() {
            state.set_text(text.to_string());
        }
        self.sync_bash_mode_from_editor();
    }

    fn clear_editor(&mut self) {
        if let Ok(mut state) = self.editor_state.lock() {
            state.clear();
        }
        self.sync_bash_mode_from_editor();
    }

    fn push_editor_history(&mut self, text: &str) {
        if let Ok(mut state) = self.editor_state.lock() {
            state.push_history(text.to_string());
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
        self.show_status("TODO: async version check hook");
        self.show_status("TODO: async package update check hook");
        self.show_status("TODO: async tmux keyboard setup check hook");
    }

    fn get_changelog_for_display(&self) -> Option<String> {
        Some("Interactive skeleton ported from pi: setup/init/run/controller state.".to_string())
    }

    async fn bind_current_session_extensions(&mut self) -> InteractiveResult<()> {
        let cwd = self.controller.runtime_host.cwd().display().to_string();
        self.show_status(format!(
            "TODO: bind session extensions into header/footer/widget containers ({cwd})"
        ));
        Ok(())
    }

    fn render_initial_messages(&mut self) {
        let reason = self
            .controller
            .runtime_host
            .session()
            .session_start_event()
            .reason
            .clone();
        let status = format!("interactive controller initialized ({reason})");
        self.render_state_mut().last_status = Some(status.clone());
        self.show_status(status);
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
        if self.loading_animation {
            self.show_status("TODO: abort queued loading state");
        } else if self.is_bash_running {
            self.is_bash_running = false;
            self.show_warning("Canceled bash placeholder run");
        } else if self
            .is_bash_mode
            .lock()
            .map(|value| *value)
            .unwrap_or(false)
        {
            self.clear_editor();
            self.set_bash_mode(false);
            self.show_status("Exited bash mode");
        } else if self.editor_text().trim().is_empty() {
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
    }

    fn handle_ctrl_c(&mut self) {
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
        self.hide_thinking_block = !self.hide_thinking_block;
        self.show_status(if self.hide_thinking_block {
            "Thinking visibility cycled to hidden"
        } else {
            "Thinking visibility cycled to visible"
        });
    }

    fn cycle_model(&mut self, direction: &str) {
        self.show_status(format!("TODO: cycle model {direction}"));
    }

    fn toggle_tool_output_expansion(&mut self) {
        self.tool_output_expanded = !self.tool_output_expanded;
        self.show_status(format!(
            "tool output expansion {}",
            if self.tool_output_expanded {
                "enabled"
            } else {
                "collapsed"
            }
        ));
    }

    fn toggle_thinking_block_visibility(&mut self) {
        self.hide_thinking_block = !self.hide_thinking_block;
        self.show_status(format!(
            "thinking block {}",
            if self.hide_thinking_block {
                "hidden"
            } else {
                "expanded"
            }
        ));
    }

    fn handle_follow_up(&mut self) {
        self.show_status("TODO: queue follow-up message");
    }

    fn handle_dequeue(&mut self) {
        self.show_status("TODO: edit queued follow-up messages");
    }

    fn handle_clipboard_image_paste(&mut self) {
        self.show_status("TODO: clipboard image paste");
    }

    fn show_settings_selector(&mut self) {
        let _ = self.controller.commands.show_settings_selector();
        self.show_placeholder("settings selector");
    }

    fn handle_model_command(&mut self, search_term: Option<&str>) {
        match search_term {
            Some(search_term) => {
                self.show_status(format!("TODO: model command search '{search_term}'"))
            }
            None => self.show_status("TODO: model command"),
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
        self.show_status(format!("TODO: rename session with '{text}'"));
    }

    fn handle_session_command(&mut self) {
        self.show_status("TODO: session command");
    }

    fn handle_changelog_command(&mut self) {
        self.show_status("TODO: changelog command");
    }

    fn handle_hotkeys_command(&mut self) {
        self.show_status("TODO: hotkeys command");
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
        self.compaction_queued_messages.clear();
        self.render_state_mut().chat_items.clear();
        self.render_state_mut().pending_items.clear();
        self.show_status("Started a fresh interactive session shell around the core session");
    }

    fn handle_compact_command(&mut self, instructions: Option<&str>) {
        self.is_compacting = true;
        self.show_status(match instructions {
            Some(instructions) => format!("TODO: compact with instructions '{instructions}'"),
            None => "TODO: compact conversation".to_string(),
        });
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
        self.pending_bash_components.push_back(format!(
            "bash{}> {}",
            if excluded_from_context {
                "(excluded)"
            } else {
                ""
            },
            command
        ));
        self.show_status("TODO: execute bash command through runtime host");
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

    fn show_placeholder(&mut self, label: &str) {
        self.show_status(format!("TODO: {label}"));
    }

    fn show_warning(&mut self, message: impl Into<String>) {
        let message = message.into();
        self.render_state_mut().last_status = Some(format!("warning: {message}"));
        self.status_lines.push(format!("warning: {message}"));
    }

    fn show_status(&mut self, message: impl Into<String>) {
        let message = message.into();
        self.render_state_mut().last_status = Some(message.clone());
        self.status_lines.push(message);
    }
}

pub async fn run_interactive(
    runtime_host: AgentSessionRuntimeHost,
    options: InteractiveModeOptions,
) -> InteractiveResult<()> {
    let mut mode = InteractiveMode::new(runtime_host, options);
    mode.run().await
}

struct SharedEditor {
    inner: Arc<Mutex<EditorComponent>>,
}

impl SharedEditor {
    fn new(inner: Arc<Mutex<EditorComponent>>) -> Self {
        Self { inner }
    }
}

impl Component for SharedEditor {
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
