use bb_core::types::AgentMessage;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// All hook event types.
#[derive(Clone, Debug)]
pub enum Event {
    SessionStart,
    SessionShutdown,
    SessionBeforeCompact(CompactPrep),
    SessionCompact {
        from_plugin: bool,
    },
    SessionBeforeTree(TreePrep),
    SessionTree {
        new_leaf: Option<String>,
        old_leaf: Option<String>,
    },
    BeforeAgentStart {
        prompt: String,
        system_prompt: String,
    },
    AgentEnd,
    TurnStart {
        turn_index: u32,
    },
    TurnEnd {
        turn_index: u32,
    },
    ToolCall(ToolCallEvent),
    ToolResult(ToolResultEvent),
    Context(ContextEvent),
    BeforeProviderRequest {
        payload: Value,
    },
    Input(InputEvent),
}

impl Event {
    pub fn event_type(&self) -> &'static str {
        match self {
            Self::SessionStart => "session_start",
            Self::SessionShutdown => "session_shutdown",
            Self::SessionBeforeCompact(_) => "session_before_compact",
            Self::SessionCompact { .. } => "session_compact",
            Self::SessionBeforeTree(_) => "session_before_tree",
            Self::SessionTree { .. } => "session_tree",
            Self::BeforeAgentStart { .. } => "before_agent_start",
            Self::AgentEnd => "agent_end",
            Self::TurnStart { .. } => "turn_start",
            Self::TurnEnd { .. } => "turn_end",
            Self::ToolCall(_) => "tool_call",
            Self::ToolResult(_) => "tool_result",
            Self::Context(_) => "context",
            Self::BeforeProviderRequest { .. } => "before_provider_request",
            Self::Input(_) => "input",
        }
    }
}

#[derive(Clone, Debug)]
pub struct CompactPrep {
    pub first_kept_entry_id: String,
    pub tokens_before: u64,
}

#[derive(Clone, Debug)]
pub struct TreePrep {
    pub target_id: String,
    pub old_leaf_id: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ToolCallEvent {
    pub tool_call_id: String,
    pub tool_name: String,
    pub input: Value,
}

#[derive(Clone, Debug)]
pub struct ToolResultEvent {
    pub tool_call_id: String,
    pub tool_name: String,
    pub input: Value,
    pub content: Vec<bb_core::types::ContentBlock>,
    pub details: Option<Value>,
    pub is_error: bool,
}

#[derive(Clone, Debug)]
pub struct ContextEvent {
    pub messages: Vec<AgentMessage>,
}

#[derive(Clone, Debug)]
pub struct InputEvent {
    pub text: String,
    pub source: String,
}

/// Result returned by a hook handler.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct HookResult {
    /// For tool_call: block execution
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block: Option<bool>,
    /// Reason for blocking/cancelling
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// For session_before_compact / session_before_tree: cancel
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cancel: Option<bool>,
    /// For context: replacement messages
    #[serde(skip_serializing_if = "Option::is_none")]
    pub messages: Option<Vec<Value>>,
    /// For before_agent_start: system prompt override
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
    /// For before_agent_start: injected message
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<Value>,
    /// For tool_result: replacement content
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<Vec<Value>>,
    /// For tool_result: replacement details
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
    /// For tool_result: replacement error flag
    #[serde(skip_serializing_if = "Option::is_none", alias = "isError")]
    pub is_error: Option<bool>,
    /// For tool_call: patched input
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<Value>,
    /// For input: action
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    /// For input: transformed text
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Generic payload for custom compaction/summary overrides
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<Value>,
}
