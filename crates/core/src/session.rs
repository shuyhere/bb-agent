//! Agent session types.
//!
//! The `AgentSession` struct implementation lives in the CLI crate
//! because it depends on `bb-session`, `bb-tools`, and `bb-provider`,
//! which themselves depend on `bb-core` (avoiding circular deps).
//!
//! This module re-exports the shared types used across the session boundary.
//!
//! Note: `AgentLoopEvent` and `ContextUsage` remain here only for transitional
//! legacy compatibility. New monitor/runtime code should prefer
//! `bb_core::agent_session_runtime` and `bb-monitor`.

#[doc(hidden)]
pub use crate::agent_loop::{AgentLoopEvent, ContextUsage};
