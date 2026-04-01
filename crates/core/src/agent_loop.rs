//! Agent loop event types.
//!
//! The actual loop implementation lives in the CLI crate (`bb-agent`)
//! because it depends on `bb-session`, `bb-tools`, and `bb-provider`,
//! which themselves depend on `bb-core` (avoiding circular deps).

/// Events emitted by the agent loop, forwarded to the UI layer.
#[derive(Clone, Debug)]
pub enum AgentLoopEvent {
    TurnStart { turn_index: u32 },
    TextDelta { text: String },
    ThinkingDelta { text: String },
    ToolCallStart { id: String, name: String },
    ToolCallDelta { id: String, args_delta: String },
    ToolExecuting { id: String, name: String },
    ToolResult { id: String, name: String, content: String, is_error: bool },
    TurnEnd { turn_index: u32 },
    AssistantDone,
    Error { message: String },
}

/// Context usage information.
#[derive(Clone, Debug)]
pub struct ContextUsage {
    pub tokens: u64,
    pub context_window: u64,
    pub percent: f64,
}
