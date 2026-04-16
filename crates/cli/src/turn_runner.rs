//! Shared streaming turn loop used by both print mode and interactive mode.
//!
//! Extracts the duplicated logic for:
//! - Building `CompletionRequest`
//! - Calling `provider.stream()`
//! - Collecting stream events
//! - Building assistant messages and appending entries to session DB
//! - Executing tool calls
//! - Looping for multi-turn tool use

use crate::extensions::ExtensionCommandRegistry;
use crate::login::ResolvedProviderAuth;
use crate::tool_registry::ToolRegistry;
use bb_core::types::ContentBlock;
use bb_monitor::RequestMetricsTracker;
use bb_provider::Provider;
use bb_provider::registry::Model;
use bb_tools::ToolContext;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

pub(crate) struct TurnConfig {
    pub conn: Arc<Mutex<rusqlite::Connection>>,
    pub session_id: String,
    pub system_prompt: String,
    pub model: Model,
    pub provider: Arc<dyn Provider>,
    pub auth: Option<ResolvedProviderAuth>,
    pub api_key: String,
    pub base_url: String,
    pub headers: std::collections::HashMap<String, String>,
    pub compaction_settings: bb_core::types::CompactionSettings,
    pub tool_registry: ToolRegistry,
    pub tool_ctx: ToolContext,
    pub thinking: Option<String>,
    pub retry_enabled: bool,
    pub retry_max_retries: u32,
    pub retry_base_delay_ms: u64,
    pub retry_max_delay_ms: u64,
    pub cancel: CancellationToken,
    pub extensions: ExtensionCommandRegistry,
    pub request_metrics_tracker: Arc<Mutex<RequestMetricsTracker>>,
    pub request_metrics_log_path: Option<PathBuf>,
}

#[derive(Clone, Debug)]
pub(crate) enum TurnEvent {
    TurnStart {
        turn_index: u32,
    },
    TextDelta(String),
    ThinkingDelta(String),
    ToolCallStart {
        id: String,
        name: String,
    },
    ToolCallDelta {
        id: String,
        args: String,
    },
    ToolExecuting {
        id: String,
    },
    ToolOutputDelta {
        id: String,
        chunk: String,
    },
    ToolResult {
        id: String,
        name: String,
        content: Vec<ContentBlock>,
        details: Option<serde_json::Value>,
        artifact_path: Option<String>,
        is_error: bool,
    },
    TurnEnd,
    ContextOverflow {
        message: String,
    },
    AutoRetryStart {
        attempt: u32,
        max_attempts: u32,
        delay_ms: u64,
        error_message: String,
    },
    AutoRetryEnd,
    AutoCompactionStart,
    Done {
        text: String,
    },
    Error(String),
    Status(String),
}

mod hooks;
mod panic;
mod persistence;
mod runner;
mod tools;

pub(crate) use persistence::{
    append_user_message_with_images, get_leaf_raw, open_sibling_conn, wrap_conn,
};
pub(crate) use runner::{run_turn, run_turn_inner};

#[cfg(test)]
mod tests;
