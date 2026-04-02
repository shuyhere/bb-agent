#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionStartEvent {
    pub reason: String,
}

impl SessionStartEvent {
    pub fn startup() -> Self {
        Self {
            reason: "startup".to_owned(),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThinkingLevel {
    #[default]
    Off,
    Low,
    Medium,
    High,
    XHigh,
}
