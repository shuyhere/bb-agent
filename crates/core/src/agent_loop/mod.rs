//! Transitional legacy agent loop compatibility surface.
//!
//! This module is retained for older integrations but is not the canonical
//! stabilized runtime API for BB-Agent. Prefer `bb_core::agent::Agent`.

mod compat;
mod runner;
mod streaming;
mod tool_execution;
mod types;

#[cfg(test)]
mod tests;

pub use compat::{is_context_overflow, is_rate_limited, messages_to_provider};
#[allow(deprecated)]
#[doc(hidden)]
pub use runner::{agent_loop, agent_loop_continue};
#[cfg(test)]
pub(crate) use types::MessageQueue;
#[doc(hidden)]
pub use types::{AgentEventStream, AgentStream};
pub use types::{AgentLoopEvent, ContextUsage};
