//! Core agent, session, configuration, and runtime types for BB-Agent.
//!
//! The primary stable runtime surface is [`agent::Agent`].
//! The `agent_loop` module is retained only as a transitional compatibility
//! layer for older integrations and is intentionally hidden from normal docs.

pub mod agent;
/// Transitional compatibility surface retained for legacy integrations.
#[doc(hidden)]
pub mod agent_loop;
pub mod agent_session;
pub mod agent_session_extensions;
pub mod agent_session_runtime;
pub mod config;
pub mod error;
pub mod session;
pub mod settings;
mod settings_defaults;
mod settings_packages;
pub mod tool_names;
pub mod types;

#[cfg(test)]
mod tool_names_tests;

// agent_session types are accessed via bb_core::agent_session::
