use super::*;

pub struct InteractiveMode {
    pub(super) controller: InteractiveController,
    pub(super) session_setup: InteractiveSessionSetup,
    pub(super) ui: TUI,
    pub(super) header_container: Arc<Mutex<Container>>,
    pub(super) chat_container: Arc<Mutex<Container>>,
    pub(super) pending_messages_container: Arc<Mutex<Container>>,
    pub(super) status_container: Arc<Mutex<Container>>,
    pub(super) widget_container_above: Arc<Mutex<Container>>,
    pub(super) widget_container_below: Arc<Mutex<Container>>,
    pub(super) footer_container: Arc<Mutex<Container>>,
    pub(super) footer_data_provider: FooterDataProvider,
    pub(super) editor: Arc<Mutex<Editor>>,
    pub(super) version: String,
    pub(super) options: InteractiveModeOptions,
    pub(super) is_initialized: bool,
    pub(super) on_input_callback: Option<Box<dyn FnMut(String) + Send>>,
    pub(super) status_loader: Option<(StatusLoaderStyle, String)>,
    pub(super) pending_working_message: Option<String>,
    pub(super) default_working_message: &'static str,
    pub(super) default_hidden_thinking_label: &'static str,
    pub(super) hidden_thinking_label: String,
    pub(super) last_sigint_time: Option<Instant>,
    pub(super) last_escape_time: Option<Instant>,
    pub(super) changelog_markdown: Option<String>,
    pub(super) tool_output_expanded: bool,
    pub(super) hide_thinking_block: bool,
    pub(super) shutdown_requested: bool,
    pub(super) is_bash_mode: Arc<Mutex<bool>>,
    pub(super) is_bash_running: bool,
    pub(super) is_streaming: bool,
    pub(super) is_compacting: bool,
    pub(super) streaming_text: String,
    pub(super) streaming_thinking: String,
    pub(super) streaming_tool_calls: Vec<ToolCallContent>,
    pub(super) pending_bash_components: VecDeque<String>,
    pub(super) steering_queue: VecDeque<String>,
    pub(super) follow_up_queue: VecDeque<String>,
    pub(super) compaction_queued_messages: VecDeque<QueuedMessage>,
    pub(super) key_handlers: Vec<(KeyBinding, KeyAction)>,
    pub(super) submit_routes: Vec<SubmitRoute>,
    pub(super) agent_events: Option<mpsc::UnboundedReceiver<AgentLoopEvent>>,
    pub(super) events: Option<UnboundedReceiver<TerminalEvent>>,
    pub(super) header_lines: Vec<String>,
    pub(super) chat_lines: Vec<String>,
    pub(super) pending_lines: Vec<String>,
    pub(super) status_lines: Vec<String>,
    pub(super) footer_lines: Vec<String>,
    pub(super) widgets_above_lines: Vec<String>,
    pub(super) widgets_below_lines: Vec<String>,
}

impl InteractiveMode {
    pub fn new(
        runtime_host: AgentSessionRuntimeHost,
        options: InteractiveModeOptions,
        session_setup: InteractiveSessionSetup,
    ) -> Self {
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

    pub(super) fn render_state(&self) -> &InteractiveRenderState {
        &self.controller.session.render_state
    }

    pub(super) fn render_state_mut(&mut self) -> &mut InteractiveRenderState {
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
