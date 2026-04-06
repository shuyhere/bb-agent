use std::sync::Arc;

use super::super::abort::AgentAbortSignal;
use super::super::callbacks::{AgentFuture, Listener};
use super::super::data::AgentMessageRole;
use super::super::events::RuntimeAgentEvent;
use super::super::helpers::{default_convert_to_llm, default_stream_fn};
use super::{
    Agent, AgentInner, AgentMessage, AgentOptions, AgentState, ContinueAction, PendingMessageQueue,
    QueueMode,
};

impl Agent {
    pub fn new(options: AgentOptions) -> Self {
        let inner = AgentInner {
            state: AgentState::from_init(options.initial_state.unwrap_or_default()),
            listeners: Vec::new(),
            next_listener_id: 0,
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
            inner: Arc::new(tokio::sync::Mutex::new(inner)),
        }
    }

    pub async fn subscribe<F>(&self, listener: F) -> impl FnOnce() + Send + 'static
    where
        F: Fn(RuntimeAgentEvent, AgentAbortSignal) -> AgentFuture<()> + Send + Sync + 'static,
    {
        let listener: Listener = Arc::new(listener);
        let mut inner = self.inner.lock().await;
        let listener_id = inner.next_listener_id;
        inner.next_listener_id += 1;
        inner.listeners.push((listener_id, listener));
        let agent = self.clone();
        move || {
            let agent = agent.clone();
            tokio::spawn(async move {
                let mut inner = agent.inner.lock().await;
                inner.listeners.retain(|(id, _)| *id != listener_id);
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
}
