//! Agent loop structure ported from pi's `packages/agent/src/agent-loop.ts`.
//!
//! This module now contains a Rust equivalent of pi's top-level loop boundaries:
//! - `agent_loop`
//! - `agent_loop_continue`
//! - `run_agent_loop`
//! - `run_agent_loop_continue`
//! - helper functions for loop execution, assistant streaming, tool preparation,
//!   tool execution, finalization, and result emission
//!
//! The concrete LLM/tool runtime in BB-Agent is still split across older layers,
//! so several parts remain TODO-safe placeholders. The architecture and function
//! boundaries are intentionally kept close to pi so later wiring can be done
//! without redesigning the module shape again.

mod runner;
mod streaming;
mod tool_execution;
pub mod types;

// ── Re-exports (preserve original public surface) ──────────────────────────

pub use runner::{agent_loop, agent_loop_continue, run_agent_loop, run_agent_loop_continue};
pub use types::{
    AgentEventStream, AgentLoopEvent, AgentStream, AgentToolCall, AgentToolResult, ContextUsage,
    LoopAssistantMessage, LoopEventSink, MessageQueue, ToolResultMessage,
};

// ── Shared internal helper ─────────────────────────────────────────────────

fn default_abort_signal() -> crate::agent::AgentAbortSignal {
    crate::agent::AgentAbortController::new().signal()
}

// ── Legacy CLI/session compatibility helpers ───────────────────────────────

/// Check if a provider error message indicates a context overflow.
///
/// This legacy string matcher is still used by the CLI/session compatibility
/// shim while the runtime loop migration is in progress.
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
///
/// The canonical implementation lives in `bb_core::agent_loop` even though the
/// underlying conversion logic is shared with `agent_session`.
pub fn messages_to_provider(messages: &[crate::types::AgentMessage]) -> Vec<serde_json::Value> {
    crate::agent_session::messages_to_provider(messages)
}

#[cfg(test)]
mod tests {
    use super::{is_context_overflow, is_rate_limited, MessageQueue};

    #[test]
    fn test_is_context_overflow() {
        assert!(is_context_overflow("HTTP 400: context_length_exceeded"));
        assert!(is_context_overflow("maximum context length is 200000 tokens"));
        assert!(is_context_overflow("too many tokens in the request"));
        assert!(is_context_overflow("request too large for model"));
        assert!(is_context_overflow("prompt is too long"));
        assert!(is_context_overflow("HTTP 400: token limit exceeded"));
        assert!(!is_context_overflow("HTTP 401: Unauthorized"));
        assert!(!is_context_overflow("HTTP 500: Internal Server Error"));
    }

    #[test]
    fn test_is_rate_limited() {
        assert!(is_rate_limited("HTTP 429: Rate limit exceeded"));
        assert!(is_rate_limited("rate limit reached"));
        assert!(is_rate_limited("429 Too Many Requests"));
        assert!(!is_rate_limited("HTTP 400: Bad request"));
        assert!(!is_rate_limited("HTTP 500: Internal Server Error"));
    }

    #[test]
    fn test_message_queue() {
        let mut q = MessageQueue::new();
        assert!(q.is_empty());

        q.push_steer("fix the bug".into());
        q.push_follow_up("then run tests".into());
        q.push_steer("also check imports".into());

        assert!(!q.is_empty());

        let steers = q.take_steers();
        assert_eq!(steers.len(), 2);
        assert_eq!(steers[0], "fix the bug");
        assert_eq!(steers[1], "also check imports");

        let follow_ups = q.take_follow_ups();
        assert_eq!(follow_ups.len(), 1);
        assert_eq!(follow_ups[0], "then run tests");

        assert!(q.is_empty());
    }

    #[test]
    fn test_message_queue_empty_operations() {
        let mut q = MessageQueue::new();
        assert!(q.take_steers().is_empty());
        assert!(q.take_follow_ups().is_empty());
        assert!(q.is_empty());
    }
}
