use super::*;

pub(super) struct UIContainers {
    pub(super) tui: TUI,
    pub(super) header_container: Arc<Mutex<Container>>,
    pub(super) chat_container: Arc<Mutex<Container>>,
    pub(super) pending_messages_container: Arc<Mutex<Container>>,
    pub(super) status_container: Arc<Mutex<Container>>,
    pub(super) widget_container_above: Arc<Mutex<Container>>,
    pub(super) widget_container_below: Arc<Mutex<Container>>,
    pub(super) footer_container: Arc<Mutex<Container>>,
    pub(super) footer_data_provider: FooterDataProvider,
    pub(super) editor: Arc<Mutex<Editor>>,
}

pub(super) struct StreamingState {
    pub(super) streaming_text: String,
    pub(super) streaming_thinking: String,
    pub(super) streaming_tool_calls: Vec<ToolCallContent>,
    pub(super) is_streaming: bool,
    pub(super) pending_working_message: Option<String>,
    pub(super) status_loader: Option<(StatusLoaderStyle, String)>,
    pub(super) hide_thinking_block: bool,
    pub(super) hidden_thinking_label: String,
    pub(super) default_working_message: &'static str,
    pub(super) default_hidden_thinking_label: &'static str,
    /// When set, the next editor submit saves an API key instead of sending a prompt.
    pub(super) pending_auth_provider: Option<String>,
    /// Sender for the manual-paste fallback during an OAuth flow.
    pub(super) pending_oauth_manual_tx: Option<tokio::sync::oneshot::Sender<String>>,
    /// Receiver for the result of a background OAuth flow.
    pub(super) pending_oauth_result_rx: Option<tokio::sync::oneshot::Receiver<Result<crate::oauth::OAuthCredentials, String>>>,
    /// Provider that just completed OAuth login and needs verification.
    pub(super) pending_oauth_verify_provider: Option<String>,
    pub(super) retry_in_progress: bool,
}

pub(super) struct QueueState {
    pub(super) steering_queue: VecDeque<String>,
    pub(super) follow_up_queue: VecDeque<String>,
    pub(super) compaction_queued_messages: VecDeque<QueuedMessage>,
    pub(super) pending_bash_components: VecDeque<String>,
}

pub(super) struct RenderCache {
    pub(super) header_lines: Vec<String>,
    pub(super) chat_lines: Vec<String>,
    pub(super) pending_lines: Vec<String>,
    pub(super) status_lines: Vec<String>,
    pub(super) footer_lines: Vec<String>,
    pub(super) widgets_above_lines: Vec<String>,
    /// Cached rendered lines for completed (non-streaming) chat items.
    pub(super) cached_chat_lines_prefix: Vec<String>,
    pub(super) cached_chat_line_count: usize,
    pub(super) cached_chat_width: u16,
    pub(super) widgets_below_lines: Vec<String>,
}

pub(super) struct InteractionState {
    pub(super) last_sigint_time: Option<Instant>,
    pub(super) last_escape_time: Option<Instant>,
    pub(super) is_bash_running: bool,
    pub(super) is_bash_mode: Arc<Mutex<bool>>,
    /// Shared flag for SIGINT handler to force exit.
    pub(super) sigint_flag: Arc<std::sync::atomic::AtomicBool>,
    pub(super) is_compacting: bool,
    pub(super) shutdown_requested: bool,
    pub(super) is_initialized: bool,
    pub(super) tool_output_expanded: bool,
    /// When true, the session selector is being used for /fork (not /resume).
    pub(super) pending_fork: bool,
}

pub struct InteractiveMode {
    pub(super) controller: InteractiveController,
    pub(super) session_setup: InteractiveSessionSetup,
    pub(super) ui: UIContainers,
    pub(super) streaming: StreamingState,
    pub(super) queues: QueueState,
    pub(super) render_cache: RenderCache,
    pub(super) interaction: InteractionState,
    pub(super) version: String,
    pub(super) options: InteractiveModeOptions,
    pub(super) on_input_callback: Option<Box<dyn FnMut(String) + Send>>,
    pub(super) changelog_markdown: Option<String>,
    pub(super) key_handlers: Vec<(KeyBinding, KeyAction)>,
    pub(super) submit_routes: Vec<SubmitRoute>,
    pub(super) agent_events: Option<mpsc::UnboundedReceiver<AgentLoopEvent>>,
    pub(super) events: Option<UnboundedReceiver<TerminalEvent>>,
    /// Shared cancellation token for the current streaming turn. Cancel from Esc/Ctrl-C.
    pub(super) abort_token: tokio_util::sync::CancellationToken,
}

impl InteractiveMode {
    pub fn new(
        runtime_host: AgentSessionRuntimeHost,
        options: InteractiveModeOptions,
        session_setup: InteractiveSessionSetup,
    ) -> Self {
        let editor = {
            let mut e = Editor::new();
            bb_tui::component::Focusable::set_focused(&mut e, true);
            Arc::new(Mutex::new(e))
        };
        let is_bash_mode = Arc::new(Mutex::new(false));
        let footer_cwd = runtime_host.cwd().to_path_buf();

        let mut this = Self {
            controller: InteractiveController::new(runtime_host),
            session_setup,
            ui: UIContainers {
                tui: TUI::new(),
                header_container: Arc::new(Mutex::new(Container::new())),
                chat_container: Arc::new(Mutex::new(Container::new())),
                pending_messages_container: Arc::new(Mutex::new(Container::new())),
                status_container: Arc::new(Mutex::new(Container::new())),
                widget_container_above: Arc::new(Mutex::new(Container::new())),
                widget_container_below: Arc::new(Mutex::new(Container::new())),
                footer_container: Arc::new(Mutex::new(Container::new())),
                footer_data_provider: FooterDataProvider::new(footer_cwd),
                editor,
            },
            streaming: StreamingState {
                is_streaming: false,
                status_loader: None,
                pending_working_message: None,
                default_working_message: "Working...",
                default_hidden_thinking_label: "Thinking...",
                hidden_thinking_label: "Thinking...".to_string(),
                hide_thinking_block: false,
                streaming_text: String::new(),
                streaming_thinking: String::new(),
                streaming_tool_calls: Vec::new(),
                pending_auth_provider: None,
                pending_oauth_manual_tx: None,
                pending_oauth_result_rx: None,
                pending_oauth_verify_provider: None,
                retry_in_progress: false,
            },
            queues: QueueState {
                steering_queue: VecDeque::new(),
                follow_up_queue: VecDeque::new(),
                compaction_queued_messages: VecDeque::new(),
                pending_bash_components: VecDeque::new(),
            },
            render_cache: RenderCache {
                header_lines: Vec::new(),
                chat_lines: Vec::new(),
                pending_lines: Vec::new(),
                status_lines: Vec::new(),
                footer_lines: Vec::new(),
                widgets_above_lines: Vec::new(),
                widgets_below_lines: Vec::new(),
                cached_chat_lines_prefix: Vec::new(),
                cached_chat_line_count: 0,
                cached_chat_width: 0,
            },
            interaction: InteractionState {
                last_sigint_time: None,
                last_escape_time: None,
                is_bash_running: false,
                is_bash_mode,
                is_compacting: false,
                shutdown_requested: false,
                is_initialized: false,
                tool_output_expanded: true,
                pending_fork: false,
                sigint_flag: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            },
            version: env!("CARGO_PKG_VERSION").to_string(),
            options,
            on_input_callback: None,
            changelog_markdown: None,
            key_handlers: Vec::new(),
            submit_routes: Vec::new(),
            agent_events: None,
            events: None,
            abort_token: tokio_util::sync::CancellationToken::new(),
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
