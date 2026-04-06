use std::sync::Arc;

use tokio::sync::{Mutex, Notify};

use super::abort::{AgentAbortController, AgentAbortSignal};
use super::callbacks::{
    AfterToolCallFn, BeforeToolCallFn, ConvertToLlmFn, GetApiKeyFn, Listener, StreamFn,
    TransformContextFn,
};
use super::data::{AgentMessage, ThinkingBudgets, ToolExecutionMode, Transport};
use super::queue::{PendingMessageQueue, QueueMode};
use super::state::{AgentState, AgentStateInit};

mod control;
mod event_processing;
mod lifecycle;
#[cfg(test)]
mod tests;

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
    pub get_api_key: Option<GetApiKeyFn>,
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
    listeners: Vec<(u64, Listener)>,
    next_listener_id: u64,
    steering_queue: PendingMessageQueue,
    follow_up_queue: PendingMessageQueue,
    active_run: Option<ActiveRun>,
    convert_to_llm: ConvertToLlmFn,
    transform_context: Option<TransformContextFn>,
    stream_fn: StreamFn,
    get_api_key: Option<GetApiKeyFn>,
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
