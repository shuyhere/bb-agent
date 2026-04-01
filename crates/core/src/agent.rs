use crate::types::*;
use serde_json::Value;
use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::{Mutex, Notify};

/// Configuration for the agent loop.
pub struct AgentConfig {
    pub system_prompt: String,
    pub model_id: String,
    pub provider_name: String,
}

/// An event emitted by the agent loop.
#[derive(Clone, Debug)]
pub enum AgentEvent {
    TurnStart {
        turn_index: u32,
    },
    AssistantText {
        text: String,
    },
    AssistantThinking {
        text: String,
    },
    ToolCallStart {
        id: String,
        name: String,
    },
    ToolCallArgs {
        id: String,
        args: Value,
    },
    ToolResult {
        id: String,
        result: String,
        is_error: bool,
    },
    TurnEnd {
        turn_index: u32,
    },
    Done,
    Error {
        message: String,
    },
}

/// A pending tool call from the assistant.
#[derive(Clone, Debug)]
pub struct PendingToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

/// Parse tool calls from assistant content.
pub fn extract_tool_calls(content: &[AssistantContent]) -> Vec<PendingToolCall> {
    content
        .iter()
        .filter_map(|c| match c {
            AssistantContent::ToolCall {
                id,
                name,
                arguments,
            } => Some(PendingToolCall {
                id: id.clone(),
                name: name.clone(),
                arguments: arguments.clone(),
            }),
            _ => None,
        })
        .collect()
}

/// Extract text from assistant content.
pub fn extract_text(content: &[AssistantContent]) -> String {
    content
        .iter()
        .filter_map(|c| match c {
            AssistantContent::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

/// Build the system prompt from base prompt + AGENTS.md content.
pub fn build_system_prompt(base: &str, agents_md: Option<&str>) -> String {
    match agents_md {
        Some(md) if !md.is_empty() => format!("{base}\n\n{md}"),
        _ => base.to_string(),
    }
}

/// The default minimal system prompt.
pub const DEFAULT_SYSTEM_PROMPT: &str = r#"You are an expert coding assistant. You help users by reading files, executing commands, editing code, and writing new files.

Available tools:
- read: Read file contents (text and images), with offset/limit for large files
- bash: Execute bash commands with optional timeout
- edit: Make precise edits with exact text replacement
- write: Create or overwrite files

Guidelines:
- Use bash for file operations like ls, grep, find
- Use read to examine files before editing
- Use edit for precise changes (old text must match exactly)
- Use write only for new files or complete rewrites
- Be concise in your responses
- Show file paths clearly when working with files"#;

pub type AgentFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;
pub type Listener =
    Arc<dyn Fn(RuntimeAgentEvent, AgentAbortSignal) -> AgentFuture<()> + Send + Sync>;
pub type ConvertToLlmFn =
    Arc<dyn Fn(Vec<AgentMessage>) -> AgentFuture<Vec<AgentMessage>> + Send + Sync>;
pub type TransformContextFn = Arc<
    dyn Fn(Vec<AgentMessage>, AgentAbortSignal) -> AgentFuture<Vec<AgentMessage>> + Send + Sync,
>;
pub type BeforeToolCallFn = Arc<
    dyn Fn(BeforeToolCallContext, AgentAbortSignal) -> AgentFuture<Option<BeforeToolCallResult>>
        + Send
        + Sync,
>;
pub type AfterToolCallFn = Arc<
    dyn Fn(AfterToolCallContext, AgentAbortSignal) -> AgentFuture<Option<AfterToolCallResult>>
        + Send
        + Sync,
>;
pub type StreamFn = Arc<
    dyn Fn(
            AgentContextSnapshot,
            AgentLoopConfig,
            AgentEventSink,
            AgentAbortSignal,
        ) -> AgentFuture<anyhow::Result<()>>
        + Send
        + Sync,
>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueueMode {
    All,
    OneAtATime,
}

impl Default for QueueMode {
    fn default() -> Self {
        Self::OneAtATime
    }
}

#[derive(Clone, Debug)]
pub struct UsageCost {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub total: u64,
}

impl Default for UsageCost {
    fn default() -> Self {
        Self {
            input: 0,
            output: 0,
            cache_read: 0,
            cache_write: 0,
            total: 0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Usage {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub total_tokens: u64,
    pub cost: UsageCost,
}

impl Default for Usage {
    fn default() -> Self {
        Self {
            input: 0,
            output: 0,
            cache_read: 0,
            cache_write: 0,
            total_tokens: 0,
            cost: UsageCost::default(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct AgentModel {
    pub id: String,
    pub name: String,
    pub api: String,
    pub provider: String,
    pub base_url: String,
    pub reasoning: bool,
    pub input: Vec<String>,
    pub cost: UsageCost,
    pub context_window: u64,
    pub max_tokens: u64,
}

impl Default for AgentModel {
    fn default() -> Self {
        Self {
            id: "unknown".to_string(),
            name: "unknown".to_string(),
            api: "unknown".to_string(),
            provider: "unknown".to_string(),
            base_url: String::new(),
            reasoning: false,
            input: Vec::new(),
            cost: UsageCost::default(),
            context_window: 0,
            max_tokens: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ThinkingLevel {
    Off,
    Low,
    Medium,
    High,
}

impl Default for ThinkingLevel {
    fn default() -> Self {
        Self::Off
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Transport {
    Sse,
    Placeholder,
}

impl Default for Transport {
    fn default() -> Self {
        Self::Sse
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToolExecutionMode {
    Parallel,
    Sequential,
}

impl Default for ToolExecutionMode {
    fn default() -> Self {
        Self::Parallel
    }
}

#[derive(Clone, Debug, Default)]
pub struct ThinkingBudgets {
    pub low: Option<u64>,
    pub medium: Option<u64>,
    pub high: Option<u64>,
}

#[derive(Clone, Debug, Default)]
pub struct AgentTool {
    pub name: String,
    pub description: Option<String>,
}

#[derive(Clone, Debug)]
pub enum AgentMessageRole {
    User,
    Assistant,
    ToolResult,
    System,
}

#[derive(Clone, Debug)]
pub enum AgentMessageContent {
    Text(String),
    Image { mime_type: String, data: Vec<u8> },
}

#[derive(Clone, Debug)]
pub struct AgentMessage {
    pub role: AgentMessageRole,
    pub content: Vec<AgentMessageContent>,
    pub api: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub usage: Option<Usage>,
    pub stop_reason: Option<String>,
    pub error_message: Option<String>,
    pub timestamp: i64,
}

impl AgentMessage {
    pub fn user_text(text: impl Into<String>) -> Self {
        Self {
            role: AgentMessageRole::User,
            content: vec![AgentMessageContent::Text(text.into())],
            api: None,
            provider: None,
            model: None,
            usage: None,
            stop_reason: None,
            error_message: None,
            timestamp: chrono::Utc::now().timestamp_millis(),
        }
    }

    pub fn assistant_error(model: &AgentModel, stop_reason: &str, error_message: String) -> Self {
        Self {
            role: AgentMessageRole::Assistant,
            content: vec![AgentMessageContent::Text(String::new())],
            api: Some(model.api.clone()),
            provider: Some(model.provider.clone()),
            model: Some(model.id.clone()),
            usage: Some(Usage::default()),
            stop_reason: Some(stop_reason.to_string()),
            error_message: Some(error_message),
            timestamp: chrono::Utc::now().timestamp_millis(),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct AgentContextSnapshot {
    pub system_prompt: String,
    pub messages: Vec<AgentMessage>,
    pub tools: Vec<AgentTool>,
}

#[derive(Clone, Debug, Default)]
pub struct BeforeToolCallContext {
    pub tool_name: Option<String>,
    pub tool_call_id: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct BeforeToolCallResult {
    pub replacement: Option<AgentMessage>,
}

#[derive(Clone, Debug, Default)]
pub struct AfterToolCallContext {
    pub tool_name: Option<String>,
    pub tool_call_id: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct AfterToolCallResult {
    pub replacement: Option<AgentMessage>,
}

#[derive(Clone)]
pub struct AgentEventSink {
    inner: Arc<dyn Fn(RuntimeAgentEvent) -> AgentFuture<anyhow::Result<()>> + Send + Sync>,
}

impl AgentEventSink {
    pub fn new<F>(f: F) -> Self
    where
        F: Fn(RuntimeAgentEvent) -> AgentFuture<anyhow::Result<()>> + Send + Sync + 'static,
    {
        Self { inner: Arc::new(f) }
    }

    pub async fn emit(&self, event: RuntimeAgentEvent) -> anyhow::Result<()> {
        (self.inner)(event).await
    }
}

#[derive(Clone, Debug)]
pub enum RuntimeAgentEvent {
    MessageStart { message: AgentMessage },
    MessageUpdate { message: AgentMessage },
    MessageEnd { message: AgentMessage },
    ToolExecutionStart { tool_call_id: String },
    ToolExecutionEnd { tool_call_id: String },
    TurnEnd { message: AgentMessage },
    AgentEnd { messages: Vec<AgentMessage> },
}

#[derive(Clone, Debug)]
pub struct AgentState {
    pub system_prompt: String,
    pub model: AgentModel,
    pub thinking_level: ThinkingLevel,
    pub tools: Vec<AgentTool>,
    pub messages: Vec<AgentMessage>,
    pub is_streaming: bool,
    pub streaming_message: Option<AgentMessage>,
    pub pending_tool_calls: HashSet<String>,
    pub error_message: Option<String>,
}

impl Default for AgentState {
    fn default() -> Self {
        Self {
            system_prompt: String::new(),
            model: AgentModel::default(),
            thinking_level: ThinkingLevel::Off,
            tools: Vec::new(),
            messages: Vec::new(),
            is_streaming: false,
            streaming_message: None,
            pending_tool_calls: HashSet::new(),
            error_message: None,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct AgentStateInit {
    pub system_prompt: Option<String>,
    pub model: Option<AgentModel>,
    pub thinking_level: Option<ThinkingLevel>,
    pub tools: Option<Vec<AgentTool>>,
    pub messages: Option<Vec<AgentMessage>>,
}

impl AgentState {
    pub fn from_init(initial: AgentStateInit) -> Self {
        Self {
            system_prompt: initial.system_prompt.unwrap_or_default(),
            model: initial.model.unwrap_or_default(),
            thinking_level: initial.thinking_level.unwrap_or_default(),
            tools: initial.tools.unwrap_or_default(),
            messages: initial.messages.unwrap_or_default(),
            is_streaming: false,
            streaming_message: None,
            pending_tool_calls: HashSet::new(),
            error_message: None,
        }
    }
}

#[derive(Clone, Default)]
pub struct AgentLoopConfig {
    pub model: AgentModel,
    pub reasoning: Option<ThinkingLevel>,
    pub session_id: Option<String>,
    pub transport: Transport,
    pub thinking_budgets: Option<ThinkingBudgets>,
    pub max_retry_delay_ms: Option<u64>,
    pub tool_execution: ToolExecutionMode,
    pub before_tool_call: Option<BeforeToolCallFn>,
    pub after_tool_call: Option<AfterToolCallFn>,
    pub convert_to_llm: Option<ConvertToLlmFn>,
    pub transform_context: Option<TransformContextFn>,
    pub get_api_key: Option<Arc<dyn Fn(String) -> AgentFuture<Option<String>> + Send + Sync>>,
    pub get_steering_messages:
        Option<Arc<dyn Fn() -> AgentFuture<Vec<AgentMessage>> + Send + Sync>>,
    pub get_follow_up_messages:
        Option<Arc<dyn Fn() -> AgentFuture<Vec<AgentMessage>> + Send + Sync>>,
}

#[derive(Clone)]
pub struct AgentAbortSignal {
    state: Arc<AbortState>,
}

struct AbortState {
    aborted: std::sync::atomic::AtomicBool,
    notify: Notify,
}

impl AgentAbortSignal {
    pub fn aborted(&self) -> bool {
        self.state.aborted.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub async fn cancelled(&self) {
        if self.aborted() {
            return;
        }
        self.state.notify.notified().await;
    }
}

#[derive(Clone)]
pub struct AgentAbortController {
    state: Arc<AbortState>,
}

impl AgentAbortController {
    pub fn new() -> Self {
        Self {
            state: Arc::new(AbortState {
                aborted: std::sync::atomic::AtomicBool::new(false),
                notify: Notify::new(),
            }),
        }
    }

    pub fn signal(&self) -> AgentAbortSignal {
        AgentAbortSignal {
            state: Arc::clone(&self.state),
        }
    }

    pub fn abort(&self) {
        self.state
            .aborted
            .store(true, std::sync::atomic::Ordering::SeqCst);
        self.state.notify.notify_waiters();
    }
}

#[derive(Clone, Debug)]
pub struct PendingMessageQueue {
    mode: QueueMode,
    messages: Vec<AgentMessage>,
}

impl PendingMessageQueue {
    pub fn new(mode: QueueMode) -> Self {
        Self {
            mode,
            messages: Vec::new(),
        }
    }

    pub fn mode(&self) -> QueueMode {
        self.mode
    }

    pub fn set_mode(&mut self, mode: QueueMode) {
        self.mode = mode;
    }

    pub fn enqueue(&mut self, message: AgentMessage) {
        self.messages.push(message);
    }

    pub fn has_items(&self) -> bool {
        !self.messages.is_empty()
    }

    pub fn drain(&mut self) -> Vec<AgentMessage> {
        match self.mode {
            QueueMode::All => std::mem::take(&mut self.messages),
            QueueMode::OneAtATime => {
                if self.messages.is_empty() {
                    Vec::new()
                } else {
                    vec![self.messages.remove(0)]
                }
            }
        }
    }

    pub fn clear(&mut self) {
        self.messages.clear();
    }
}

#[derive(Clone)]
struct ActiveRun {
    signal: AgentAbortSignal,
    controller: AgentAbortController,
    done: Arc<Notify>,
}

impl ActiveRun {
    fn new() -> Self {
        let controller = AgentAbortController::new();
        let signal = controller.signal();
        Self {
            signal,
            controller,
            done: Arc::new(Notify::new()),
        }
    }

    async fn wait(&self) {
        self.done.notified().await;
    }

    fn finish(&self) {
        self.done.notify_waiters();
    }
}

#[derive(Clone, Default)]
pub struct AgentOptions {
    pub initial_state: Option<AgentStateInit>,
    pub convert_to_llm: Option<ConvertToLlmFn>,
    pub transform_context: Option<TransformContextFn>,
    pub stream_fn: Option<StreamFn>,
    pub get_api_key: Option<Arc<dyn Fn(String) -> AgentFuture<Option<String>> + Send + Sync>>,
    pub before_tool_call: Option<BeforeToolCallFn>,
    pub after_tool_call: Option<AfterToolCallFn>,
    pub steering_mode: Option<QueueMode>,
    pub follow_up_mode: Option<QueueMode>,
    pub session_id: Option<String>,
    pub thinking_budgets: Option<ThinkingBudgets>,
    pub transport: Option<Transport>,
    pub max_retry_delay_ms: Option<u64>,
    pub tool_execution: Option<ToolExecutionMode>,
}

struct AgentInner {
    state: AgentState,
    listeners: Vec<Listener>,
    steering_queue: PendingMessageQueue,
    follow_up_queue: PendingMessageQueue,
    active_run: Option<ActiveRun>,
    convert_to_llm: ConvertToLlmFn,
    transform_context: Option<TransformContextFn>,
    stream_fn: StreamFn,
    get_api_key: Option<Arc<dyn Fn(String) -> AgentFuture<Option<String>> + Send + Sync>>,
    before_tool_call: Option<BeforeToolCallFn>,
    after_tool_call: Option<AfterToolCallFn>,
    session_id: Option<String>,
    thinking_budgets: Option<ThinkingBudgets>,
    transport: Transport,
    max_retry_delay_ms: Option<u64>,
    tool_execution: ToolExecutionMode,
}

#[derive(Clone)]
pub struct Agent {
    inner: Arc<Mutex<AgentInner>>,
}

impl Agent {
    pub fn new(options: AgentOptions) -> Self {
        let inner = AgentInner {
            state: AgentState::from_init(options.initial_state.unwrap_or_default()),
            listeners: Vec::new(),
            steering_queue: PendingMessageQueue::new(options.steering_mode.unwrap_or_default()),
            follow_up_queue: PendingMessageQueue::new(options.follow_up_mode.unwrap_or_default()),
            active_run: None,
            convert_to_llm: options
                .convert_to_llm
                .unwrap_or_else(default_convert_to_llm),
            transform_context: options.transform_context,
            stream_fn: options.stream_fn.unwrap_or_else(default_stream_fn),
            get_api_key: options.get_api_key,
            before_tool_call: options.before_tool_call,
            after_tool_call: options.after_tool_call,
            session_id: options.session_id,
            thinking_budgets: options.thinking_budgets,
            transport: options.transport.unwrap_or_default(),
            max_retry_delay_ms: options.max_retry_delay_ms,
            tool_execution: options.tool_execution.unwrap_or_default(),
        };
        Self {
            inner: Arc::new(Mutex::new(inner)),
        }
    }

    pub async fn subscribe<F>(&self, listener: F) -> impl FnOnce() + Send + 'static
    where
        F: Fn(RuntimeAgentEvent, AgentAbortSignal) -> AgentFuture<()> + Send + Sync + 'static,
    {
        let listener: Listener = Arc::new(listener);
        let mut inner = self.inner.lock().await;
        inner.listeners.push(listener);
        let index = inner.listeners.len() - 1;
        let agent = self.clone();
        move || {
            let agent = agent.clone();
            tokio::spawn(async move {
                let mut inner = agent.inner.lock().await;
                if index < inner.listeners.len() {
                    inner.listeners.remove(index);
                }
            });
        }
    }

    pub async fn state(&self) -> AgentState {
        self.inner.lock().await.state.clone()
    }

    pub async fn set_steering_mode(&self, mode: QueueMode) {
        self.inner.lock().await.steering_queue.set_mode(mode);
    }

    pub async fn steering_mode(&self) -> QueueMode {
        self.inner.lock().await.steering_queue.mode()
    }

    pub async fn set_follow_up_mode(&self, mode: QueueMode) {
        self.inner.lock().await.follow_up_queue.set_mode(mode);
    }

    pub async fn follow_up_mode(&self) -> QueueMode {
        self.inner.lock().await.follow_up_queue.mode()
    }

    pub async fn steer(&self, message: AgentMessage) {
        self.inner.lock().await.steering_queue.enqueue(message);
    }

    pub async fn follow_up(&self, message: AgentMessage) {
        self.inner.lock().await.follow_up_queue.enqueue(message);
    }

    pub async fn clear_steering_queue(&self) {
        self.inner.lock().await.steering_queue.clear();
    }

    pub async fn clear_follow_up_queue(&self) {
        self.inner.lock().await.follow_up_queue.clear();
    }

    pub async fn clear_all_queues(&self) {
        let mut inner = self.inner.lock().await;
        inner.steering_queue.clear();
        inner.follow_up_queue.clear();
    }

    pub async fn has_queued_messages(&self) -> bool {
        let inner = self.inner.lock().await;
        inner.steering_queue.has_items() || inner.follow_up_queue.has_items()
    }

    pub async fn signal(&self) -> Option<AgentAbortSignal> {
        self.inner
            .lock()
            .await
            .active_run
            .as_ref()
            .map(|run| run.signal.clone())
    }

    pub async fn abort(&self) {
        if let Some(run) = self.inner.lock().await.active_run.clone() {
            run.controller.abort();
        }
    }

    pub async fn wait_for_idle(&self) {
        let active = self.inner.lock().await.active_run.clone();
        if let Some(active) = active {
            active.wait().await;
        }
    }

    pub async fn reset(&self) {
        let mut inner = self.inner.lock().await;
        inner.state.messages.clear();
        inner.state.is_streaming = false;
        inner.state.streaming_message = None;
        inner.state.pending_tool_calls.clear();
        inner.state.error_message = None;
        inner.steering_queue.clear();
        inner.follow_up_queue.clear();
    }

    pub async fn prompt_text(&self, input: impl Into<String>) -> anyhow::Result<()> {
        self.prompt_messages(vec![AgentMessage::user_text(input.into())])
            .await
    }

    pub async fn prompt_message(&self, message: AgentMessage) -> anyhow::Result<()> {
        self.prompt_messages(vec![message]).await
    }

    pub async fn prompt_messages(&self, messages: Vec<AgentMessage>) -> anyhow::Result<()> {
        {
            let inner = self.inner.lock().await;
            if inner.active_run.is_some() {
                anyhow::bail!(
                    "Agent is already processing a prompt. Use steer() or follow_up() to queue messages, or wait for completion."
                );
            }
        }
        self.run_prompt_messages(messages, false).await
    }

    pub async fn continue_run(&self) -> anyhow::Result<()> {
        let decision = {
            let mut inner = self.inner.lock().await;
            if inner.active_run.is_some() {
                anyhow::bail!(
                    "Agent is already processing. Wait for completion before continuing."
                );
            }

            let Some(last_message) = inner.state.messages.last().cloned() else {
                anyhow::bail!("No messages to continue from");
            };

            match last_message.role {
                AgentMessageRole::Assistant => {
                    let queued_steering = inner.steering_queue.drain();
                    if !queued_steering.is_empty() {
                        ContinueAction::Prompt {
                            messages: queued_steering,
                            skip_initial_steering_poll: true,
                        }
                    } else {
                        let queued_follow_ups = inner.follow_up_queue.drain();
                        if !queued_follow_ups.is_empty() {
                            ContinueAction::Prompt {
                                messages: queued_follow_ups,
                                skip_initial_steering_poll: false,
                            }
                        } else {
                            anyhow::bail!("Cannot continue from message role: assistant");
                        }
                    }
                }
                AgentMessageRole::User | AgentMessageRole::ToolResult => ContinueAction::Continue,
                AgentMessageRole::System => {
                    anyhow::bail!("Cannot continue from message role: system")
                }
            }
        };

        match decision {
            ContinueAction::Prompt {
                messages,
                skip_initial_steering_poll,
            } => {
                self.run_prompt_messages(messages, skip_initial_steering_poll)
                    .await
            }
            ContinueAction::Continue => self.run_continuation().await,
        }
    }

    async fn run_prompt_messages(
        &self,
        messages: Vec<AgentMessage>,
        skip_initial_steering_poll: bool,
    ) -> anyhow::Result<()> {
        let messages_for_run = messages;
        self.run_with_lifecycle(move |agent, signal| {
            let messages = messages_for_run.clone();
            Box::pin(async move {
                let context = agent.create_context_snapshot().await;
                let config = agent
                    .create_loop_config(LoopConfigOptions {
                        skip_initial_steering_poll,
                    })
                    .await;
                let sink = agent.event_sink();
                let stream_fn = { agent.inner.lock().await.stream_fn.clone() };
                (stream_fn)(context_with_prompt(context, messages), config, sink, signal).await
            })
        })
        .await
    }

    async fn run_continuation(&self) -> anyhow::Result<()> {
        self.run_with_lifecycle(|agent, signal| {
            Box::pin(async move {
                let context = agent.create_context_snapshot().await;
                let config = agent.create_loop_config(LoopConfigOptions::default()).await;
                let sink = agent.event_sink();
                let stream_fn = { agent.inner.lock().await.stream_fn.clone() };
                (stream_fn)(context, config, sink, signal).await
            })
        })
        .await
    }

    async fn create_context_snapshot(&self) -> AgentContextSnapshot {
        let inner = self.inner.lock().await;
        AgentContextSnapshot {
            system_prompt: inner.state.system_prompt.clone(),
            messages: inner.state.messages.clone(),
            tools: inner.state.tools.clone(),
        }
    }

    async fn create_loop_config(&self, options: LoopConfigOptions) -> AgentLoopConfig {
        let inner = self.inner.lock().await;
        let skip_initial_steering_poll = Arc::new(std::sync::atomic::AtomicBool::new(
            options.skip_initial_steering_poll,
        ));
        let steering_agent = self.clone();
        let follow_up_agent = self.clone();
        AgentLoopConfig {
            model: inner.state.model.clone(),
            reasoning: match inner.state.thinking_level {
                ThinkingLevel::Off => None,
                level => Some(level),
            },
            session_id: inner.session_id.clone(),
            transport: inner.transport,
            thinking_budgets: inner.thinking_budgets.clone(),
            max_retry_delay_ms: inner.max_retry_delay_ms,
            tool_execution: inner.tool_execution,
            before_tool_call: inner.before_tool_call.clone(),
            after_tool_call: inner.after_tool_call.clone(),
            convert_to_llm: Some(inner.convert_to_llm.clone()),
            transform_context: inner.transform_context.clone(),
            get_api_key: inner.get_api_key.clone(),
            get_steering_messages: Some(Arc::new(move || {
                let agent = steering_agent.clone();
                let skip_flag = skip_initial_steering_poll.clone();
                Box::pin(async move {
                    let should_skip = skip_flag.swap(false, std::sync::atomic::Ordering::SeqCst);
                    if should_skip {
                        Vec::new()
                    } else {
                        let mut inner = agent.inner.lock().await;
                        inner.steering_queue.drain()
                    }
                })
            })),
            get_follow_up_messages: Some(Arc::new(move || {
                let agent = follow_up_agent.clone();
                Box::pin(async move {
                    let mut inner = agent.inner.lock().await;
                    inner.follow_up_queue.drain()
                })
            })),
        }
    }

    async fn run_with_lifecycle<F>(&self, executor: F) -> anyhow::Result<()>
    where
        F: FnOnce(Agent, AgentAbortSignal) -> AgentFuture<anyhow::Result<()>> + Send + 'static,
    {
        let active_run = {
            let mut inner = self.inner.lock().await;
            if inner.active_run.is_some() {
                anyhow::bail!("Agent is already processing.");
            }
            let active_run = ActiveRun::new();
            inner.active_run = Some(active_run.clone());
            inner.state.is_streaming = true;
            inner.state.streaming_message = None;
            inner.state.error_message = None;
            active_run
        };

        let result = executor(self.clone(), active_run.signal.clone()).await;
        if let Err(error) = result {
            self.handle_run_failure(error, active_run.signal.aborted())
                .await?;
        }
        self.finish_run().await;
        Ok(())
    }

    async fn handle_run_failure(&self, error: anyhow::Error, aborted: bool) -> anyhow::Result<()> {
        let failure_message = {
            let inner = self.inner.lock().await;
            AgentMessage::assistant_error(
                &inner.state.model,
                if aborted { "aborted" } else { "error" },
                error.to_string(),
            )
        };

        {
            let mut inner = self.inner.lock().await;
            inner.state.messages.push(failure_message.clone());
            inner.state.error_message = failure_message.error_message.clone();
        }

        self.process_event(RuntimeAgentEvent::AgentEnd {
            messages: vec![failure_message],
        })
        .await
    }

    async fn finish_run(&self) {
        let active_run = {
            let mut inner = self.inner.lock().await;
            inner.state.is_streaming = false;
            inner.state.streaming_message = None;
            inner.state.pending_tool_calls.clear();
            inner.active_run.take()
        };

        if let Some(active_run) = active_run {
            active_run.finish();
        }
    }

    async fn process_event(&self, event: RuntimeAgentEvent) -> anyhow::Result<()> {
        let (listeners, signal) = {
            let mut inner = self.inner.lock().await;
            match &event {
                RuntimeAgentEvent::MessageStart { message } => {
                    inner.state.streaming_message = Some(message.clone());
                }
                RuntimeAgentEvent::MessageUpdate { message } => {
                    inner.state.streaming_message = Some(message.clone());
                }
                RuntimeAgentEvent::MessageEnd { message } => {
                    inner.state.streaming_message = None;
                    inner.state.messages.push(message.clone());
                }
                RuntimeAgentEvent::ToolExecutionStart { tool_call_id } => {
                    inner.state.pending_tool_calls.insert(tool_call_id.clone());
                }
                RuntimeAgentEvent::ToolExecutionEnd { tool_call_id } => {
                    inner.state.pending_tool_calls.remove(tool_call_id);
                }
                RuntimeAgentEvent::TurnEnd { message } => {
                    if matches!(message.role, AgentMessageRole::Assistant) {
                        if let Some(error_message) = &message.error_message {
                            inner.state.error_message = Some(error_message.clone());
                        }
                    }
                }
                RuntimeAgentEvent::AgentEnd { .. } => {
                    inner.state.streaming_message = None;
                }
            }

            let Some(active_run) = inner.active_run.clone() else {
                anyhow::bail!("Agent listener invoked outside active run");
            };
            (inner.listeners.clone(), active_run.signal)
        };

        for listener in listeners {
            listener(event.clone(), signal.clone()).await;
        }
        Ok(())
    }

    fn event_sink(&self) -> AgentEventSink {
        let agent = self.clone();
        AgentEventSink::new(move |event| {
            let agent = agent.clone();
            Box::pin(async move { agent.process_event(event).await })
        })
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct LoopConfigOptions {
    skip_initial_steering_poll: bool,
}

enum ContinueAction {
    Prompt {
        messages: Vec<AgentMessage>,
        skip_initial_steering_poll: bool,
    },
    Continue,
}

fn context_with_prompt(
    mut context: AgentContextSnapshot,
    messages: Vec<AgentMessage>,
) -> AgentContextSnapshot {
    context.messages.extend(messages);
    context
}

fn default_convert_to_llm() -> ConvertToLlmFn {
    Arc::new(|messages| {
        Box::pin(async move {
            messages
                .into_iter()
                .filter(|message| {
                    matches!(
                        message.role,
                        AgentMessageRole::User
                            | AgentMessageRole::Assistant
                            | AgentMessageRole::ToolResult
                    )
                })
                .collect()
        })
    })
}

fn default_stream_fn() -> StreamFn {
    Arc::new(|_context, _config, _sink, _signal| {
        Box::pin(async move {
            anyhow::bail!(
                "Agent::stream_fn is not implemented in bb-core yet; provide a runtime loop placeholder"
            )
        })
    })
}
