use crate::agent_session_extensions::SessionStartReason;

pub use crate::types::ThinkingLevel;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionStartEvent {
    pub reason: SessionStartReason,
}

impl SessionStartEvent {
    pub fn startup() -> Self {
        Self {
            reason: SessionStartReason::Startup,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopedModel {
    pub model: ModelRef,
    pub thinking_level: Option<ThinkingLevel>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelRef {
    pub provider: String,
    pub id: String,
    pub reasoning: bool,
}
