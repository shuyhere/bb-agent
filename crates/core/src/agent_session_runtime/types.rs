use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::agent_session::{ModelRef, ScopedModel, SessionStartEvent, ThinkingLevel};
use crate::agent_session_extensions::SessionResourceBootstrap;

#[derive(Debug, Error)]
pub enum AgentSessionRuntimeError {
    #[error("no model selected")]
    NoModelSelected,
    #[error("already compacted")]
    AlreadyCompacted,
    #[error("nothing to compact (session too small)")]
    NothingToCompact,
    #[error("entry {0} not found")]
    EntryNotFound(String),
    #[error("no model available for summarization")]
    NoModelForSummarization,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeEntrySource {
    #[default]
    Runtime,
    Extension,
}

impl RuntimeEntrySource {
    pub fn is_extension(self) -> bool {
        matches!(self, Self::Extension)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum RuntimeEvent {
    CompactionStart {
        reason: CompactionReason,
    },
    CompactionEnd {
        reason: CompactionReason,
        result: Option<CompactionResult>,
        aborted: bool,
        will_retry: bool,
        error_message: Option<String>,
    },
    AutoRetryStart {
        attempt: u32,
        max_attempts: u32,
        delay_ms: u64,
        error_message: String,
    },
    AutoRetryEnd {
        success: bool,
        attempt: u32,
        final_error: Option<String>,
    },
    SessionTree {
        new_leaf_id: Option<String>,
        old_leaf_id: Option<String>,
        summary_entry_id: Option<String>,
        summary_source: Option<RuntimeEntrySource>,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum CompactionReason {
    Manual,
    Overflow,
    Threshold,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompactionResult {
    pub summary: String,
    pub first_kept_entry_id: String,
    pub tokens_before: usize,
    pub details: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompactionSettings {
    pub enabled: bool,
    pub threshold_percent: u8,
    pub reserve_tokens: usize,
    pub min_entries: usize,
    pub keep_recent_entries: usize,
}

impl Default for CompactionSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            threshold_percent: 80,
            reserve_tokens: 2_048,
            min_entries: 8,
            keep_recent_entries: 6,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RetrySettings {
    pub enabled: bool,
    pub max_retries: u32,
    pub base_delay_ms: u64,
}

impl Default for RetrySettings {
    fn default() -> Self {
        Self {
            enabled: true,
            max_retries: 3,
            base_delay_ms: 1_000,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeModelRef {
    pub provider: String,
    pub id: String,
    pub context_window: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct RuntimeCost {
    pub total_microunits: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct RuntimeUsage {
    pub input: usize,
    pub output: usize,
    pub cache_read: usize,
    pub cache_write: usize,
    pub cost: RuntimeCost,
}

impl RuntimeUsage {
    pub fn total_context_tokens(&self) -> usize {
        self.input + self.output + self.cache_read + self.cache_write
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AssistantStopReason {
    Completed,
    Error,
    Aborted,
    Other(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MessagePart {
    Text { text: String },
    ToolCall { id: String, name: String },
    Other { kind: String, text: Option<String> },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum RuntimeMessage {
    User {
        content: Vec<MessagePart>,
        timestamp: DateTime<Utc>,
    },
    Assistant(AssistantMessage),
    ToolResult {
        content: String,
        timestamp: DateTime<Utc>,
    },
    BashExecution(BashExecutionMessage),
    Custom {
        content: Vec<MessagePart>,
        timestamp: DateTime<Utc>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AssistantMessage {
    pub content: Vec<MessagePart>,
    pub timestamp: DateTime<Utc>,
    pub stop_reason: AssistantStopReason,
    pub usage: RuntimeUsage,
    pub error_message: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BashExecutionMessage {
    pub command: String,
    pub output: String,
    pub exit_code: i32,
    pub cancelled: bool,
    pub truncated: bool,
    pub full_output_path: Option<String>,
    pub timestamp: DateTime<Utc>,
    pub exclude_from_context: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BashResult {
    pub output: String,
    pub exit_code: i32,
    pub cancelled: bool,
    pub truncated: bool,
    pub full_output_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StoredCompactionEntry {
    pub summary: String,
    pub first_kept_entry_id: String,
    pub tokens_before: usize,
    pub details: Option<Value>,
    pub from_extension: bool,
}

impl StoredCompactionEntry {
    pub fn source(&self) -> RuntimeEntrySource {
        if self.from_extension {
            RuntimeEntrySource::Extension
        } else {
            RuntimeEntrySource::Runtime
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BranchSummaryEntry {
    pub summary: String,
    pub details: Option<Value>,
    pub from_extension: bool,
}

impl BranchSummaryEntry {
    pub fn source(&self) -> RuntimeEntrySource {
        if self.from_extension {
            RuntimeEntrySource::Extension
        } else {
            RuntimeEntrySource::Runtime
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LabelChangeEntry {
    pub target_entry_id: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SessionTreeEntryKind {
    Message(RuntimeMessage),
    CustomMessage { content: Vec<MessagePart> },
    Compaction(StoredCompactionEntry),
    BranchSummary(BranchSummaryEntry),
    LabelChange(LabelChangeEntry),
    Placeholder { name: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionTreeEntry {
    pub id: String,
    pub parent_id: Option<String>,
    pub timestamp: DateTime<Utc>,
    pub kind: SessionTreeEntryKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TreePreparation {
    pub target_id: String,
    pub old_leaf_id: Option<String>,
    pub common_ancestor_id: Option<String>,
    pub entries_to_summarize: Vec<String>,
    pub user_wants_summary: bool,
    pub custom_instructions: Option<String>,
    pub replace_instructions: Option<bool>,
    pub label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct NavigateTreeOptions {
    pub summarize: bool,
    pub custom_instructions: Option<String>,
    pub replace_instructions: Option<bool>,
    pub label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NavigateTreeOutcome {
    pub editor_text: Option<String>,
    pub cancelled: bool,
    pub aborted: bool,
    pub summary_entry: Option<SessionTreeEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompactionPreparation {
    pub path_entry_ids: Vec<String>,
    pub first_kept_entry_id: String,
    pub entries_to_summarize: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContextEstimate {
    pub tokens: usize,
    pub last_usage_index: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContextUsage {
    pub tokens: Option<usize>,
    pub context_window: usize,
    pub percent: Option<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum CompactionAction {
    None,
    RecoverOverflow { preparation: CompactionPreparation },
    CompactForThreshold { preparation: CompactionPreparation },
    OverflowRecoveryFailed { message: String },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum AbortedMessageBehavior {
    #[default]
    Ignore,
    Consider,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct CompactionCheckOptions {
    pub aborted_message_behavior: AbortedMessageBehavior,
}

impl CompactionCheckOptions {
    pub fn should_ignore_aborted_message(self) -> bool {
        matches!(
            self.aborted_message_behavior,
            AbortedMessageBehavior::Ignore
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum RetryAction {
    None,
    Scheduled {
        attempt: u32,
        delay_ms: u64,
    },
    MaxRetriesExceeded {
        attempts: u32,
        final_error: Option<String>,
    },
    Disabled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum RetryCompletion {
    Succeeded,
    Failed { final_error: Option<String> },
}

impl RetryCompletion {
    pub fn was_successful(&self) -> bool {
        matches!(self, Self::Succeeded)
    }

    pub fn final_error(&self) -> Option<&str> {
        match self {
            Self::Succeeded => None,
            Self::Failed { final_error } => final_error.as_deref(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum BashContextPolicy {
    #[default]
    Include,
    Exclude,
}

impl BashContextPolicy {
    pub fn exclude_from_context(self) -> bool {
        matches!(self, Self::Exclude)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum BashMessageDelivery {
    #[default]
    AppendImmediately,
    StreamPending,
}

impl BashMessageDelivery {
    pub fn should_buffer(self) -> bool {
        matches!(self, Self::StreamPending)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PreparedBashCommand {
    pub(in crate::agent_session_runtime) original_command: String,
    pub(in crate::agent_session_runtime) resolved_command: String,
    pub(in crate::agent_session_runtime) cwd: String,
    pub(in crate::agent_session_runtime) context_policy: BashContextPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct BashExecutionState {
    pub(in crate::agent_session_runtime) running_command: Option<String>,
    pub(in crate::agent_session_runtime) abort_requested: bool,
    pub(in crate::agent_session_runtime) pending_messages: Vec<BashExecutionMessage>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct RetryState {
    pub(in crate::agent_session_runtime) attempt: u32,
    pub(in crate::agent_session_runtime) in_progress: bool,
    pub(in crate::agent_session_runtime) abort_requested: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct CompactionState {
    pub(in crate::agent_session_runtime) manual_in_progress: bool,
    pub(in crate::agent_session_runtime) auto_in_progress: bool,
    pub(in crate::agent_session_runtime) overflow_recovery_attempted: bool,
    pub(in crate::agent_session_runtime) abort_requested: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct TreeNavigationState {
    pub(in crate::agent_session_runtime) branch_summary_in_progress: bool,
    pub(in crate::agent_session_runtime) abort_requested: bool,
}

#[derive(Debug, Clone, Default)]
pub struct AgentSessionRuntimeBootstrap {
    pub cwd: Option<PathBuf>,
    pub model: Option<ModelRef>,
    pub thinking_level: Option<ThinkingLevel>,
    pub scoped_models: Vec<ScopedModel>,
    pub initial_active_tool_names: Option<Vec<String>>,
    pub resource_bootstrap: SessionResourceBootstrap,
}

#[derive(Debug, Clone)]
pub struct CreateAgentSessionRuntimeOptions {
    pub cwd: PathBuf,
    pub session_start_event: Option<SessionStartEvent>,
}

impl CreateAgentSessionRuntimeOptions {
    pub fn new(cwd: impl Into<PathBuf>) -> Self {
        Self {
            cwd: cwd.into(),
            session_start_event: None,
        }
    }
}
