use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// =============================================================================
// Entry identifiers
// =============================================================================

/// 8-character hex entry identifier, unique within a session.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EntryId(pub String);

impl EntryId {
    pub fn generate() -> Self {
        let u = Uuid::new_v4();
        Self(u.simple().to_string()[..8].to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for EntryId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

// =============================================================================
// Content blocks
// =============================================================================

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image")]
    Image { data: String, mime_type: String },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AssistantContent {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "thinking")]
    Thinking { thinking: String },
    #[serde(rename = "toolCall")]
    ToolCall {
        id: String,
        name: String,
        arguments: serde_json::Value,
    },
}

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

// =============================================================================
// Session entry types
// =============================================================================

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EntryBase {
    pub id: EntryId,
    pub parent_id: Option<EntryId>,
    pub timestamp: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SessionEntry {
    #[serde(rename = "message")]
    Message {
        #[serde(flatten)]
        base: EntryBase,
        message: AgentMessage,
    },
    #[serde(rename = "compaction")]
    Compaction {
        #[serde(flatten)]
        base: EntryBase,
        summary: String,
        first_kept_entry_id: EntryId,
        tokens_before: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        details: Option<serde_json::Value>,
        #[serde(default)]
        from_plugin: bool,
    },
    #[serde(rename = "branch_summary")]
    BranchSummary {
        #[serde(flatten)]
        base: EntryBase,
        from_id: EntryId,
        summary: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        details: Option<serde_json::Value>,
        #[serde(default)]
        from_plugin: bool,
    },
    #[serde(rename = "model_change")]
    ModelChange {
        #[serde(flatten)]
        base: EntryBase,
        provider: String,
        model_id: String,
    },
    #[serde(rename = "thinking_level_change")]
    ThinkingLevelChange {
        #[serde(flatten)]
        base: EntryBase,
        thinking_level: String,
    },
    #[serde(rename = "custom")]
    Custom {
        #[serde(flatten)]
        base: EntryBase,
        custom_type: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        data: Option<serde_json::Value>,
    },
    #[serde(rename = "custom_message")]
    CustomMessage {
        #[serde(flatten)]
        base: EntryBase,
        custom_type: String,
        content: Vec<ContentBlock>,
        display: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        details: Option<serde_json::Value>,
    },
    #[serde(rename = "session_info")]
    SessionInfo {
        #[serde(flatten)]
        base: EntryBase,
        name: Option<String>,
    },
    #[serde(rename = "label")]
    Label {
        #[serde(flatten)]
        base: EntryBase,
        target_id: EntryId,
        label: Option<String>,
    },
}

impl SessionEntry {
    pub fn base(&self) -> &EntryBase {
        match self {
            Self::Message { base, .. }
            | Self::Compaction { base, .. }
            | Self::BranchSummary { base, .. }
            | Self::ModelChange { base, .. }
            | Self::ThinkingLevelChange { base, .. }
            | Self::Custom { base, .. }
            | Self::CustomMessage { base, .. }
            | Self::SessionInfo { base, .. }
            | Self::Label { base, .. } => base,
        }
    }

    pub fn entry_type(&self) -> &str {
        match self {
            Self::Message { .. } => "message",
            Self::Compaction { .. } => "compaction",
            Self::BranchSummary { .. } => "branch_summary",
            Self::ModelChange { .. } => "model_change",
            Self::ThinkingLevelChange { .. } => "thinking_level_change",
            Self::Custom { .. } => "custom",
            Self::CustomMessage { .. } => "custom_message",
            Self::SessionInfo { .. } => "session_info",
            Self::Label { .. } => "label",
        }
    }
}

// =============================================================================
// Session context (what gets sent to LLM)
// =============================================================================

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ThinkingLevel {
    Off,
    Minimal,
    Low,
    Medium,
    High,
}

impl Default for ThinkingLevel {
    fn default() -> Self {
        Self::Off
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModelInfo {
    pub provider: String,
    pub model_id: String,
}

#[derive(Clone, Debug)]
pub struct SessionContext {
    pub messages: Vec<AgentMessage>,
    pub thinking_level: ThinkingLevel,
    pub model: Option<ModelInfo>,
}

// =============================================================================
// Compaction settings
// =============================================================================

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CompactionSettings {
    pub enabled: bool,
    pub reserve_tokens: u64,
    pub keep_recent_tokens: u64,
}

impl Default for CompactionSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            reserve_tokens: 16384,
            keep_recent_tokens: 20000,
        }
    }
}

// =============================================================================
// Session header (for JSONL compat)
// =============================================================================

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionHeader {
    pub version: u32,
    pub id: Uuid,
    pub timestamp: DateTime<Utc>,
    pub cwd: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_session: Option<String>,
}
