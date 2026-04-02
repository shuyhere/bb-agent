/// Legacy CLI/session compatibility helpers used during runtime loop migration.

/// Check if a provider error message indicates a context overflow.
pub fn is_context_overflow(msg: &str) -> bool {
    let msg_lower = msg.to_lowercase();
    msg_lower.contains("context_length_exceeded")
        || msg_lower.contains("maximum context length")
        || msg_lower.contains("too many tokens")
        || msg_lower.contains("request too large")
        || msg_lower.contains("prompt is too long")
        || (msg_lower.contains("400") && msg_lower.contains("token"))
}

/// Check if a provider error message indicates rate limiting.
pub fn is_rate_limited(msg: &str) -> bool {
    msg.contains("429") || msg.to_lowercase().contains("rate limit")
}

/// Convert legacy session messages into provider request JSON.
pub fn messages_to_provider(messages: &[crate::types::AgentMessage]) -> Vec<serde_json::Value> {
    crate::agent_session::messages_to_provider(messages)
}

pub(super) fn default_abort_signal() -> crate::agent::AgentAbortSignal {
    crate::agent::AgentAbortController::new().signal()
}
