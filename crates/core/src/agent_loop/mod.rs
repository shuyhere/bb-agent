mod compat;
mod runner;
mod streaming;
mod tool_execution;
pub mod types;

#[cfg(test)]
mod tests;

pub use compat::{is_context_overflow, is_rate_limited, messages_to_provider};
pub use runner::{agent_loop, agent_loop_continue, run_agent_loop, run_agent_loop_continue};
pub use types::{
    AgentEventStream, AgentLoopEvent, AgentStream, AgentToolCall, AgentToolResult, ContextUsage,
    LoopAssistantMessage, LoopEventSink, MessageQueue, ToolResultMessage,
};
