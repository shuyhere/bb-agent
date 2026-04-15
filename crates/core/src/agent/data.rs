//! Legacy compatibility data types for the transitional `bb_core::agent` API.
//!
//! These shapes are retained for older integrations, but they are not the
//! canonical monitoring or persisted-usage vocabulary for modern BB-Agent
//! surfaces.
//!
//! Prefer:
//! - `bb_core::types::{Cost, Usage}` for persisted transcript/session usage
//! - `bb_monitor::*` for derived monitor summaries and request metrics
//! - `bb_core::agent_session_runtime::ContextUsage` for runtime context usage
//!
//! Keep this module compatibility-focused and avoid growing new monitor logic here.

use std::sync::Arc;

use chrono::Utc;

pub use crate::types::ThinkingLevel;

use super::callbacks::{
    AfterToolCallFn, BeforeToolCallFn, ConvertToLlmFn, GetApiKeyFn, TransformContextFn,
};

/// Configuration for the agent loop.
pub struct AgentConfig {
    pub system_prompt: String,
    pub model_id: String,
    pub provider_name: String,
}

/// Legacy compatibility cost shape used by the transitional `bb_core::agent`
/// surface. This is not the canonical persisted/session cost model.
#[derive(Clone, Debug, Default)]
pub struct UsageCost {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub total: u64,
}

/// Legacy compatibility usage shape used by the transitional `bb_core::agent`
/// surface. This is not the canonical persisted/session usage model.
#[derive(Clone, Debug, Default)]
pub struct Usage {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub total_tokens: u64,
    pub cost: UsageCost,
}

#[derive(Clone, Debug)]
pub struct AgentModel {
    pub id: String,
    pub name: String,
    pub api: String,
    pub provider: String,
    pub base_url: String,
    pub reasoning: bool,
    pub input: Vec<String>,
    pub cost: UsageCost,
    pub context_window: u64,
    pub max_tokens: u64,
}

impl Default for AgentModel {
    fn default() -> Self {
        Self {
            id: "unknown".to_string(),
            name: "unknown".to_string(),
            api: "unknown".to_string(),
            provider: "unknown".to_string(),
            base_url: String::new(),
            reasoning: false,
            input: Vec::new(),
            cost: UsageCost::default(),
            context_window: 0,
            max_tokens: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Default)]
pub enum Transport {
    #[default]
    Sse,
    Placeholder,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Default)]
pub enum ToolExecutionMode {
    #[default]
    Parallel,
    Sequential,
}

#[derive(Clone, Debug, Default)]
pub struct ThinkingBudgets {
    pub low: Option<u64>,
    pub medium: Option<u64>,
    pub high: Option<u64>,
}

#[derive(Clone, Debug, Default)]
pub struct AgentTool {
    pub name: String,
    pub description: Option<String>,
}

#[derive(Clone, Debug)]
pub enum AgentMessageRole {
    User,
    Assistant,
    ToolResult,
    System,
}

#[derive(Clone, Debug)]
pub enum AgentMessageContent {
    Text(String),
    Image { mime_type: String, data: Vec<u8> },
}

#[derive(Clone, Debug)]
pub struct AgentMessage {
    pub role: AgentMessageRole,
    pub content: Vec<AgentMessageContent>,
    pub api: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub usage: Option<Usage>,
    pub stop_reason: Option<String>,
    pub error_message: Option<String>,
    pub timestamp: i64,
}

impl AgentMessage {
    pub fn user_text(text: impl Into<String>) -> Self {
        Self {
            role: AgentMessageRole::User,
            content: vec![AgentMessageContent::Text(text.into())],
            api: None,
            provider: None,
            model: None,
            usage: None,
            stop_reason: None,
            error_message: None,
            timestamp: Utc::now().timestamp_millis(),
        }
    }

    pub fn assistant_error(model: &AgentModel, stop_reason: &str, error_message: String) -> Self {
        Self {
            role: AgentMessageRole::Assistant,
            content: vec![AgentMessageContent::Text(String::new())],
            api: Some(model.api.clone()),
            provider: Some(model.provider.clone()),
            model: Some(model.id.clone()),
            usage: Some(Usage::default()),
            stop_reason: Some(stop_reason.to_string()),
            error_message: Some(error_message),
            timestamp: Utc::now().timestamp_millis(),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct AgentContextSnapshot {
    pub system_prompt: String,
    pub messages: Vec<AgentMessage>,
    pub tools: Vec<AgentTool>,
}

#[derive(Clone, Debug, Default)]
pub struct BeforeToolCallContext {
    pub tool_name: Option<String>,
    pub tool_call_id: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct BeforeToolCallResult {
    pub replacement: Option<AgentMessage>,
}

#[derive(Clone, Debug, Default)]
pub struct AfterToolCallContext {
    pub tool_name: Option<String>,
    pub tool_call_id: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct AfterToolCallResult {
    pub replacement: Option<AgentMessage>,
}

#[derive(Clone, Default)]
pub struct AgentLoopConfig {
    pub model: AgentModel,
    pub reasoning: Option<ThinkingLevel>,
    pub session_id: Option<String>,
    pub transport: Transport,
    pub thinking_budgets: Option<ThinkingBudgets>,
    pub max_retry_delay_ms: Option<u64>,
    pub tool_execution: ToolExecutionMode,
    pub before_tool_call: Option<BeforeToolCallFn>,
    pub after_tool_call: Option<AfterToolCallFn>,
    pub convert_to_llm: Option<ConvertToLlmFn>,
    pub transform_context: Option<TransformContextFn>,
    pub get_api_key: Option<GetApiKeyFn>,
    pub get_steering_messages:
        Option<Arc<dyn Fn() -> super::callbacks::AgentFuture<Vec<AgentMessage>> + Send + Sync>>,
    pub get_follow_up_messages:
        Option<Arc<dyn Fn() -> super::callbacks::AgentFuture<Vec<AgentMessage>> + Send + Sync>>,
}
