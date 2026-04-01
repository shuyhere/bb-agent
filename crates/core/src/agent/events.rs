use std::sync::Arc;

use serde_json::Value;

use super::callbacks::AgentFuture;
use super::data::AgentMessage;

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
