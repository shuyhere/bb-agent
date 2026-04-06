//! Builtin tool implementations and tool integration types for BB-Agent.

mod artifacts;
pub mod bash;
pub mod browser_fetch;
mod diff;
pub mod edit;
pub mod find;
pub mod grep;
pub mod ls;
pub(crate) mod path;
pub mod read;
mod registry;
pub(crate) mod support;
pub(crate) mod text;
mod types;
pub(crate) mod web;
pub mod web_fetch;
pub mod web_search;
pub mod write;

pub use registry::builtin_tools;
pub use types::{Tool, ToolContext, ToolResult, WebSearchRuntime};
