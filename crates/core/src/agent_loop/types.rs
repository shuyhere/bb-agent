//! Data types for the agent loop: events, messages, tool call/result structs.

use crate::agent::{AgentMessage, AgentMessageContent, RuntimeAgentEvent};
use serde_json::Value;
use tokio::sync::{mpsc, oneshot};

/// Legacy UI-facing event type still used by existing BB-Agent layers.
#[derive(Clone, Debug)]
pub enum AgentLoopEvent {
    TurnStart {
        turn_index: u32,
    },
    TextDelta {
        text: String,
    },
    ThinkingDelta {
        text: String,
    },
    ToolCallStart {
        id: String,
        name: String,
    },
    ToolCallDelta {
        id: String,
        args_delta: String,
    },
    ToolExecuting {
        id: String,
        name: String,
    },
    ToolResult {
        id: String,
        name: String,
        content: Vec<crate::types::ContentBlock>,
        details: Option<serde_json::Value>,
        artifact_path: Option<String>,
        is_error: bool,
    },
    TurnEnd {
        turn_index: u32,
    },
    AutoRetryStart {
        attempt: u32,
        max_attempts: u32,
        delay_ms: u64,
        error_message: String,
    },
    AutoRetryEnd {
        success: bool,
        attempt: u32,
        final_error: Option<String>,
    },
    AssistantDone,
    Error {
        message: String,
    },
}

/// Legacy context usage information still used by existing BB-Agent layers.
#[derive(Clone, Debug)]
pub struct ContextUsage {
    pub tokens: u64,
    pub context_window: u64,
    pub percent: f64,
}

/// Legacy event stream replacement for Rust.
pub struct AgentEventStream<TEvent, TResult> {
    receiver: mpsc::UnboundedReceiver<TEvent>,
    result: oneshot::Receiver<TResult>,
}

impl<TEvent, TResult> AgentEventStream<TEvent, TResult> {
    pub fn new(
        receiver: mpsc::UnboundedReceiver<TEvent>,
        result: oneshot::Receiver<TResult>,
    ) -> Self {
        Self { receiver, result }
    }

    pub async fn recv(&mut self) -> Option<TEvent> {
        self.receiver.recv().await
    }

    pub async fn result(self) -> std::result::Result<TResult, oneshot::error::RecvError> {
        self.result.await
    }
}

pub type AgentStream = AgentEventStream<RuntimeAgentEvent, Vec<AgentMessage>>;

#[derive(Clone, Debug, Default)]
pub(crate) struct AgentToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct AgentToolResult {
    pub content: Vec<AgentMessageContent>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ToolResultMessage {
    pub content: Vec<AgentMessageContent>,
    pub is_error: bool,
    pub timestamp: i64,
}

#[derive(Clone, Debug)]
pub(crate) struct LoopAssistantMessage {
    pub message: AgentMessage,
    pub tool_calls: Vec<AgentToolCall>,
    pub stop_reason: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct PreparedToolCall {
    pub tool_call: AgentToolCall,
    pub tool: crate::agent::AgentTool,
    pub args: Value,
}

#[derive(Clone, Debug)]
pub(crate) struct ImmediateToolCallOutcome {
    pub result: AgentToolResult,
    pub is_error: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct ExecutedToolCallOutcome {
    pub result: AgentToolResult,
    pub is_error: bool,
}

/// Minimal legacy steering/follow-up queue retained only for legacy tests.
#[cfg(test)]
#[derive(Debug, Default)]
pub(crate) struct MessageQueue {
    steers: Vec<String>,
    follow_ups: Vec<String>,
}

#[cfg(test)]
impl MessageQueue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push_steer(&mut self, text: String) {
        self.steers.push(text);
    }

    pub fn push_follow_up(&mut self, text: String) {
        self.follow_ups.push(text);
    }

    pub fn take_steers(&mut self) -> Vec<String> {
        std::mem::take(&mut self.steers)
    }

    pub fn take_follow_ups(&mut self) -> Vec<String> {
        std::mem::take(&mut self.follow_ups)
    }

    pub fn is_empty(&self) -> bool {
        self.steers.is_empty() && self.follow_ups.is_empty()
    }
}
