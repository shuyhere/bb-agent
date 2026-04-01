use std::sync::Arc;

use tokio::sync::{Mutex, Notify};

use super::abort::{AgentAbortController, AgentAbortSignal};
use super::callbacks::{
    AfterToolCallFn, AgentFuture, BeforeToolCallFn, ConvertToLlmFn, Listener, StreamFn,
    TransformContextFn,
};
use super::data::{
    AgentContextSnapshot, AgentLoopConfig, AgentMessage, AgentMessageRole, ThinkingBudgets,
    ThinkingLevel, ToolExecutionMode, Transport,
};
use super::events::{AgentEventSink, RuntimeAgentEvent};
use super::helpers::{context_with_prompt, default_convert_to_llm, default_stream_fn};
use super::queue::{PendingMessageQueue, QueueMode};
use super::state::{AgentState, AgentStateInit};

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
