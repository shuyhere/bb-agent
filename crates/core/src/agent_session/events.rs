use super::config::{CustomMessageDelivery, StreamingBehavior};
use super::messages::{CustomMessage, SessionMessage, UserMessage};
use super::models::{ModelRef, SessionStartEvent, ThinkingLevel};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueState {
    pub steering: Vec<String>,
    pub follow_up: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentSessionEvent {
    QueueUpdate {
        steering: Vec<String>,
        follow_up: Vec<String>,
    },
    PromptDispatched {
        messages: Vec<SessionMessage>,
    },
    UserMessageQueued {
        delivery: StreamingBehavior,
        message: UserMessage,
    },
    CustomMessageQueued {
        delivery: CustomMessageDelivery,
        message: CustomMessage,
    },
    MessageStart {
        message: SessionMessage,
    },
    MessageEnd {
        message: SessionMessage,
    },
    ModelChanged {
        model: ModelRef,
        source: ModelChangeSource,
    },
    ThinkingLevelChanged {
        level: ThinkingLevel,
    },
    BashMessagesFlushed,
    SessionStarted {
        event: SessionStartEvent,
    },
    SessionShutdown,
    ExtensionCommandExecuted {
        command: String,
        args: Option<String>,
    },
}

pub type AgentSessionEventListener = Box<dyn Fn(&AgentSessionEvent) + Send + Sync + 'static>;
pub type Callback0 = Box<dyn Fn() + Send + Sync + 'static>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SubscriptionHandle {
    index: usize,
}

impl SubscriptionHandle {
    pub(super) fn new(index: usize) -> Self {
        Self { index }
    }

    pub(super) fn index(self) -> usize {
        self.index
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelChangeSource {
    Set,
    Cycle,
    Restore,
}
