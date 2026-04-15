//! Builtin tool implementations and tool integration types for BB-Agent.

mod artifacts;
pub mod bash;
pub mod bash_policy;
pub mod browser_fetch;
mod diff;
pub mod edit;
pub mod find;
pub mod grep;
pub mod ls;
pub(crate) mod path;
pub mod read;
mod registry;
pub(crate) mod sandbox;
pub mod scheduler;
pub(crate) mod support;
pub(crate) mod text;
mod types;
pub(crate) mod web;
pub mod web_fetch;
pub mod web_search;
pub mod write;

pub use registry::builtin_tools;
pub use scheduler::{
    FileQueue, FileQueueReservation, execute_reserved_tool_call, execute_tool_call,
    execute_tool_calls,
};
pub use types::{
    ExecutionPolicy, RequestToolApprovalFn, Tool, ToolApprovalDecision, ToolApprovalOutcome,
    ToolApprovalRequest, ToolContext, ToolExecutionMode, ToolResult, ToolScheduling,
    WebSearchRuntime,
};
