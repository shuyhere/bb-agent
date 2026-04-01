use super::components;
use super::events::{
    ChatItem, InteractiveMessage, InteractiveRenderState, PendingMessages,
    QueuedMessage as RenderQueuedMessage, QueuedMessageMode, ToolCallContent,
    assistant_message_from_parts,
};
use super::interactive_commands::SelectorKind;
use super::model_selector_overlay::{ModelSelectorOverlay, ModelSelectorOverlayAction};
use super::status_loader::{StatusLoaderComponent, StatusLoaderStyle};
use super::types::{
    InteractiveController, InteractiveModeOptions, InteractiveResult, InteractiveSessionSetup,
    KeyAction, KeyBinding, QueuedMessage, QueuedMessageKind, SubmitAction, SubmitMatch,
    SubmitOutcome, SubmitRoute,
};
use bb_core::agent_loop::AgentLoopEvent;
use bb_core::agent_session::{ModelRef, PromptOptions, ThinkingLevel};
use bb_core::agent_session_runtime::{AgentSessionRuntimeHost, RuntimeModelRef};
use bb_provider::registry::{ApiType, Model, ModelRegistry};
use bb_session::{compaction, store};
use bb_tui::component::{Component, Container, Focusable, Spacer, Text};
use bb_tui::editor::Editor;
use bb_tui::footer::{Footer, FooterData, FooterDataProvider};
use bb_tui::model_selector::ModelSelector;
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

mod actions;
mod agent_events;
mod rendering;
mod runtime;
mod shared;

use shared::{SharedContainer, SharedEditorWrapper};

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
    footer_data_provider: FooterDataProvider,
    editor: Arc<Mutex<Editor>>,
    version: String,
    options: InteractiveModeOptions,
    is_initialized: bool,
    on_input_callback: Option<Box<dyn FnMut(String) + Send>>,
    status_loader: Option<(StatusLoaderStyle, String)>,
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
        let footer_cwd = runtime_host.cwd().to_path_buf();

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
            footer_data_provider: FooterDataProvider::new(footer_cwd),
            editor,
            version: env!("CARGO_PKG_VERSION").to_string(),
            options,
            is_initialized: false,
            on_input_callback: None,
            status_loader: None,
            pending_working_message: None,
            default_working_message: "Working...",
            default_hidden_thinking_label: "Thinking...",
            hidden_thinking_label: "Thinking...".to_string(),
            last_sigint_time: None,
            last_escape_time: None,
            changelog_markdown: None,
            tool_output_expanded: true,
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
}

pub async fn run_interactive(
    runtime_host: AgentSessionRuntimeHost,
    options: InteractiveModeOptions,
    session_setup: InteractiveSessionSetup,
) -> InteractiveResult<()> {
    let mut mode = InteractiveMode::new(runtime_host, options, session_setup);
    mode.run().await
}

