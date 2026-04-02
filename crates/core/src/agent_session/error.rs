use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentSessionError {
    AlreadyProcessing,
    NoModelSelected,
    ExtensionCommandCannotBeQueued,
}

impl fmt::Display for AgentSessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AgentSessionError::AlreadyProcessing => {
                write!(
                    f,
                    "agent is already processing; choose steer or follow-up delivery"
                )
            }
            AgentSessionError::NoModelSelected => write!(f, "no model selected"),
            AgentSessionError::ExtensionCommandCannotBeQueued => {
                write!(f, "extension command cannot be queued")
            }
        }
    }
}

impl std::error::Error for AgentSessionError {}
