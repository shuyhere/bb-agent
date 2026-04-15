use bb_core::types::{AgentMessage, ContentBlock};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Hook events emitted by BB-Agent.
///
/// The variants in this enum are the stable entry points extensions can subscribe to.
/// Payload-bearing variants use dedicated structs so event-specific data can evolve
/// behind accessor methods instead of exposing every field directly.
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
    ToolExecutionStart(ToolExecutionStartEvent),
    ToolExecutionUpdate(ToolExecutionUpdateEvent),
    ToolCall(ToolCallEvent),
    ToolResult(ToolResultEvent),
    ToolExecutionEnd(ToolExecutionEndEvent),
    Context(ContextEvent),
    BeforeProviderRequest {
        payload: Value,
    },
    Input(InputEvent),
}

impl Event {
    /// Return the wire-level event type string exposed to extensions.
    #[must_use]
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
            Self::ToolExecutionStart(_) => "tool_execution_start",
            Self::ToolExecutionUpdate(_) => "tool_execution_update",
            Self::ToolCall(_) => "tool_call",
            Self::ToolResult(_) => "tool_result",
            Self::ToolExecutionEnd(_) => "tool_execution_end",
            Self::Context(_) => "context",
            Self::BeforeProviderRequest { .. } => "before_provider_request",
            Self::Input(_) => "input",
        }
    }
}

/// Compaction preparation details.
///
/// `first_kept_entry_id` identifies the first session entry that will remain after
/// compaction. `tokens_before` captures the estimated context size immediately before
/// the compaction starts.
#[derive(Clone, Debug)]
pub struct CompactPrep {
    first_kept_entry_id: String,
    tokens_before: u64,
}

impl CompactPrep {
    #[must_use]
    pub fn new(first_kept_entry_id: impl Into<String>, tokens_before: u64) -> Self {
        Self {
            first_kept_entry_id: first_kept_entry_id.into(),
            tokens_before,
        }
    }

    #[must_use]
    pub fn first_kept_entry_id(&self) -> &str {
        &self.first_kept_entry_id
    }

    #[must_use]
    pub fn tokens_before(&self) -> u64 {
        self.tokens_before
    }
}

/// Tree-switch preparation details.
#[derive(Clone, Debug)]
pub struct TreePrep {
    target_id: String,
    old_leaf_id: Option<String>,
}

impl TreePrep {
    #[must_use]
    pub fn new(target_id: impl Into<String>, old_leaf_id: Option<String>) -> Self {
        Self {
            target_id: target_id.into(),
            old_leaf_id,
        }
    }

    #[must_use]
    pub fn target_id(&self) -> &str {
        &self.target_id
    }

    #[must_use]
    pub fn old_leaf_id(&self) -> Option<&str> {
        self.old_leaf_id.as_deref()
    }
}

/// Tool-execution start payload.
#[derive(Clone, Debug)]
pub struct ToolExecutionStartEvent {
    tool_call_id: String,
    tool_name: String,
    input: Value,
}

impl ToolExecutionStartEvent {
    #[must_use]
    pub fn new(
        tool_call_id: impl Into<String>,
        tool_name: impl Into<String>,
        input: Value,
    ) -> Self {
        Self {
            tool_call_id: tool_call_id.into(),
            tool_name: tool_name.into(),
            input,
        }
    }

    #[must_use]
    pub fn tool_call_id(&self) -> &str {
        &self.tool_call_id
    }

    #[must_use]
    pub fn tool_name(&self) -> &str {
        &self.tool_name
    }

    #[must_use]
    pub fn input(&self) -> &Value {
        &self.input
    }
}

/// Tool-execution update payload.
#[derive(Clone, Debug)]
pub struct ToolExecutionUpdateEvent {
    tool_call_id: String,
    tool_name: String,
    input: Value,
    partial_result: Value,
}

impl ToolExecutionUpdateEvent {
    #[must_use]
    pub fn new(
        tool_call_id: impl Into<String>,
        tool_name: impl Into<String>,
        input: Value,
        partial_result: Value,
    ) -> Self {
        Self {
            tool_call_id: tool_call_id.into(),
            tool_name: tool_name.into(),
            input,
            partial_result,
        }
    }

    #[must_use]
    pub fn tool_call_id(&self) -> &str {
        &self.tool_call_id
    }

    #[must_use]
    pub fn tool_name(&self) -> &str {
        &self.tool_name
    }

    #[must_use]
    pub fn input(&self) -> &Value {
        &self.input
    }

    #[must_use]
    pub fn partial_result(&self) -> &Value {
        &self.partial_result
    }
}

/// Tool-call event payload.
#[derive(Clone, Debug)]
pub struct ToolCallEvent {
    tool_call_id: String,
    tool_name: String,
    input: Value,
}

impl ToolCallEvent {
    #[must_use]
    pub fn new(
        tool_call_id: impl Into<String>,
        tool_name: impl Into<String>,
        input: Value,
    ) -> Self {
        Self {
            tool_call_id: tool_call_id.into(),
            tool_name: tool_name.into(),
            input,
        }
    }

    #[must_use]
    pub fn tool_call_id(&self) -> &str {
        &self.tool_call_id
    }

    #[must_use]
    pub fn tool_name(&self) -> &str {
        &self.tool_name
    }

    #[must_use]
    pub fn input(&self) -> &Value {
        &self.input
    }
}

/// Tool-result event payload.
#[derive(Clone, Debug)]
pub struct ToolResultEvent {
    tool_call_id: String,
    tool_name: String,
    input: Value,
    content: Vec<ContentBlock>,
    details: Option<Value>,
    is_error: bool,
}

impl ToolResultEvent {
    #[must_use]
    pub fn new(
        tool_call_id: impl Into<String>,
        tool_name: impl Into<String>,
        input: Value,
        content: Vec<ContentBlock>,
        details: Option<Value>,
        is_error: bool,
    ) -> Self {
        Self {
            tool_call_id: tool_call_id.into(),
            tool_name: tool_name.into(),
            input,
            content,
            details,
            is_error,
        }
    }

    #[must_use]
    pub fn tool_call_id(&self) -> &str {
        &self.tool_call_id
    }

    #[must_use]
    pub fn tool_name(&self) -> &str {
        &self.tool_name
    }

    #[must_use]
    pub fn input(&self) -> &Value {
        &self.input
    }

    #[must_use]
    pub fn content(&self) -> &[ContentBlock] {
        &self.content
    }

    #[must_use]
    pub fn details(&self) -> Option<&Value> {
        self.details.as_ref()
    }

    #[must_use]
    pub fn is_error(&self) -> bool {
        self.is_error
    }
}

/// Tool-execution end payload.
#[derive(Clone, Debug)]
pub struct ToolExecutionEndEvent {
    tool_call_id: String,
    tool_name: String,
    input: Value,
    content: Vec<ContentBlock>,
    details: Option<Value>,
    is_error: bool,
}

impl ToolExecutionEndEvent {
    #[must_use]
    pub fn new(
        tool_call_id: impl Into<String>,
        tool_name: impl Into<String>,
        input: Value,
        content: Vec<ContentBlock>,
        details: Option<Value>,
        is_error: bool,
    ) -> Self {
        Self {
            tool_call_id: tool_call_id.into(),
            tool_name: tool_name.into(),
            input,
            content,
            details,
            is_error,
        }
    }

    #[must_use]
    pub fn tool_call_id(&self) -> &str {
        &self.tool_call_id
    }

    #[must_use]
    pub fn tool_name(&self) -> &str {
        &self.tool_name
    }

    #[must_use]
    pub fn input(&self) -> &Value {
        &self.input
    }

    #[must_use]
    pub fn content(&self) -> &[ContentBlock] {
        &self.content
    }

    #[must_use]
    pub fn details(&self) -> Option<&Value> {
        self.details.as_ref()
    }

    #[must_use]
    pub fn is_error(&self) -> bool {
        self.is_error
    }
}

/// Conversation context snapshot sent to context hooks.
///
/// The payload contains the full message list BB-Agent is about to send into the
/// provider pipeline. Extensions that replace `HookResult::messages` should preserve
/// the order and JSON shape expected by `AgentMessage` deserialization.
#[derive(Clone, Debug)]
pub struct ContextEvent {
    messages: Vec<AgentMessage>,
}

impl ContextEvent {
    #[must_use]
    pub fn new(messages: Vec<AgentMessage>) -> Self {
        Self { messages }
    }

    #[must_use]
    pub fn messages(&self) -> &[AgentMessage] {
        &self.messages
    }

    #[must_use]
    pub fn message_count(&self) -> usize {
        self.messages.len()
    }
}

/// User-input payload sent through input hooks.
#[derive(Clone, Debug)]
pub struct InputEvent {
    text: String,
    source: String,
}

impl InputEvent {
    #[must_use]
    pub fn new(text: impl Into<String>, source: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            source: source.into(),
        }
    }

    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }
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

impl HookResult {
    /// Returns `true` when the result does not request any change.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.block.is_none()
            && self.reason.is_none()
            && self.cancel.is_none()
            && self.messages.is_none()
            && self.system_prompt.is_none()
            && self.message.is_none()
            && self.content.is_none()
            && self.details.is_none()
            && self.is_error.is_none()
            && self.input.is_none()
            && self.action.is_none()
            && self.text.is_none()
            && self.payload.is_none()
    }

    /// Merge a newer hook result into this one.
    ///
    /// For each field, the last non-`None` value wins.
    pub fn merge_from(&mut self, newer: HookResult) {
        if newer.block.is_some() {
            self.block = newer.block;
        }
        if newer.reason.is_some() {
            self.reason = newer.reason;
        }
        if newer.cancel.is_some() {
            self.cancel = newer.cancel;
        }
        if newer.messages.is_some() {
            self.messages = newer.messages;
        }
        if newer.system_prompt.is_some() {
            self.system_prompt = newer.system_prompt;
        }
        if newer.message.is_some() {
            self.message = newer.message;
        }
        if newer.content.is_some() {
            self.content = newer.content;
        }
        if newer.details.is_some() {
            self.details = newer.details;
        }
        if newer.is_error.is_some() {
            self.is_error = newer.is_error;
        }
        if newer.input.is_some() {
            self.input = newer.input;
        }
        if newer.action.is_some() {
            self.action = newer.action;
        }
        if newer.text.is_some() {
            self.text = newer.text;
        }
        if newer.payload.is_some() {
            self.payload = newer.payload;
        }
    }

    /// Returns `true` when dispatch should stop after this result.
    #[must_use]
    pub fn stops_dispatch(&self) -> bool {
        self.block == Some(true) || self.cancel == Some(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn hook_result_merge_prefers_last_non_none_values() {
        let mut merged = HookResult {
            action: Some("handled".into()),
            text: Some("before".into()),
            ..Default::default()
        };

        merged.merge_from(HookResult {
            text: Some("after".into()),
            payload: Some(json!({"ok": true})),
            ..Default::default()
        });

        assert_eq!(merged.action.as_deref(), Some("handled"));
        assert_eq!(merged.text.as_deref(), Some("after"));
        assert_eq!(merged.payload, Some(json!({"ok": true})));
    }

    #[test]
    fn payload_accessors_preserve_values() {
        let execution_start =
            ToolExecutionStartEvent::new("call-1", "bash", json!({"command": "pwd"}));
        assert_eq!(execution_start.tool_call_id(), "call-1");
        assert_eq!(execution_start.tool_name(), "bash");
        assert_eq!(execution_start.input(), &json!({"command": "pwd"}));

        let execution_update = ToolExecutionUpdateEvent::new(
            "call-1",
            "bash",
            json!({"command": "pwd"}),
            json!({"details": {"schedulerState": "queued"}}),
        );
        assert_eq!(execution_update.tool_call_id(), "call-1");
        assert_eq!(
            execution_update.partial_result()["details"]["schedulerState"],
            "queued"
        );

        let tool_call = ToolCallEvent::new("call-1", "bash", json!({"command": "pwd"}));
        assert_eq!(tool_call.tool_call_id(), "call-1");
        assert_eq!(tool_call.tool_name(), "bash");
        assert_eq!(tool_call.input(), &json!({"command": "pwd"}));

        let execution_end = ToolExecutionEndEvent::new(
            "call-1",
            "bash",
            json!({"command": "pwd"}),
            vec![ContentBlock::Text { text: "ok".into() }],
            Some(json!({"durationMs": 1})),
            false,
        );
        assert_eq!(execution_end.tool_call_id(), "call-1");
        assert_eq!(execution_end.details(), Some(&json!({"durationMs": 1})));
        assert!(!execution_end.is_error());

        let input = InputEvent::new("hello", "tui");
        assert_eq!(input.text(), "hello");
        assert_eq!(input.source(), "tui");

        let compact = CompactPrep::new("entry-1", 42);
        assert_eq!(compact.first_kept_entry_id(), "entry-1");
        assert_eq!(compact.tokens_before(), 42);

        let tree = TreePrep::new("leaf-2", Some("leaf-1".into()));
        assert_eq!(tree.target_id(), "leaf-2");
        assert_eq!(tree.old_leaf_id(), Some("leaf-1"));
    }
}
