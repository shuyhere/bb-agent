mod accessors;
mod prompting;

use super::state::AgentSessionState;

/// Main session object for the bb-core public API.
///
/// Runtime integrations that depend on the concrete agent loop, session
/// persistence, settings, model registry, and extension system are layered on
/// top of this core session type.
pub struct AgentSession {
    pub(super) state: AgentSessionState,
}
