use serde::{Deserialize, Serialize};

use super::content::{AssistantContent, ContentBlock};

// =============================================================================
// Usage / cost tracking
// =============================================================================

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Cost {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_write: f64,
    pub total: f64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CacheMetricsSource {
    #[default]
    Unknown,
    Official,
    Estimated,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Usage {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub total_tokens: u64,
    pub cost: Cost,
}

// =============================================================================
// Stop reason
// =============================================================================

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StopReason {
    Stop,
    Length,
    ToolUse,
    Error,
    Aborted,
}

// =============================================================================
// Message types
// =============================================================================

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "role")]
pub enum AgentMessage {
    #[serde(rename = "user")]
    User(UserMessage),
    #[serde(rename = "assistant")]
    Assistant(AssistantMessage),
    #[serde(rename = "toolResult")]
    ToolResult(ToolResultMessage),
    #[serde(rename = "bashExecution")]
    BashExecution(BashExecutionMessage),
    #[serde(rename = "custom")]
    Custom(CustomMessage),
    #[serde(rename = "branchSummary")]
    BranchSummary(BranchSummaryMessage),
    #[serde(rename = "compactionSummary")]
    CompactionSummary(CompactionSummaryMessage),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UserMessage {
    pub content: Vec<ContentBlock>,
    pub timestamp: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AssistantMessage {
    pub content: Vec<AssistantContent>,
    pub provider: String,
    pub model: String,
    pub usage: Usage,
    pub stop_reason: StopReason,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    pub timestamp: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolResultMessage {
    pub tool_call_id: String,
    pub tool_name: String,
    pub content: Vec<ContentBlock>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
    pub is_error: bool,
    pub timestamp: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BashExecutionMessage {
    pub command: String,
    pub output: String,
    pub exit_code: Option<i32>,
    pub cancelled: bool,
    pub truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub full_output_path: Option<String>,
    pub timestamp: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CustomMessage {
    pub custom_type: String,
    pub content: Vec<ContentBlock>,
    pub display: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
    pub timestamp: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BranchSummaryMessage {
    pub summary: String,
    pub from_id: String,
    pub timestamp: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CompactionSummaryMessage {
    pub summary: String,
    pub tokens_before: u64,
    pub timestamp: i64,
}
