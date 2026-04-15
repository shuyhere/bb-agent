use bb_core::types::{AgentMessage, AssistantContent, AssistantMessage, SessionEntry};
use serde::{Deserialize, Serialize};

use crate::usage::UsageTotals;

/// Reusable session-level metrics derived from persisted semantic messages.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SessionMetricsSummary {
    pub user_messages: u64,
    pub assistant_messages: u64,
    pub tool_calls: u64,
    pub tool_results: u64,
    pub total_messages: u64,
    pub usage: UsageTotals,
}

impl SessionMetricsSummary {
    pub fn observe_entry(&mut self, entry: &SessionEntry) {
        if let SessionEntry::Message { message, .. } = entry {
            self.observe_message(message);
        }
    }

    pub fn observe_message(&mut self, message: &AgentMessage) {
        self.total_messages += 1;
        match message {
            AgentMessage::User(_) => self.user_messages += 1,
            AgentMessage::Assistant(message) => self.observe_assistant_message(message),
            AgentMessage::ToolResult(_) => self.tool_results += 1,
            _ => {}
        }
    }

    fn observe_assistant_message(&mut self, message: &AssistantMessage) {
        self.assistant_messages += 1;
        self.tool_calls += message
            .content
            .iter()
            .filter(|content| matches!(content, AssistantContent::ToolCall { .. }))
            .count() as u64;
        self.usage.input_tokens += message.usage.input;
        self.usage.output_tokens += message.usage.output;
        self.usage.cache_read_tokens += message.usage.cache_read;
        self.usage.cache_write_tokens += message.usage.cache_write;
        self.usage.total_cost += message.usage.cost.total;
        self.usage.total_tokens = self.usage.effective_total_tokens();
    }
}

pub fn collect_session_metrics<'a, I>(entries: I) -> SessionMetricsSummary
where
    I: IntoIterator<Item = &'a SessionEntry>,
{
    let mut summary = SessionMetricsSummary::default();
    for entry in entries {
        summary.observe_entry(entry);
    }
    summary
}

#[cfg(test)]
mod tests {
    use super::{SessionMetricsSummary, collect_session_metrics};
    use bb_core::types::{
        AgentMessage, AssistantContent, AssistantMessage, Cost, EntryBase, EntryId, SessionEntry,
        StopReason, ToolResultMessage, Usage, UserMessage,
    };
    use chrono::Utc;

    fn assistant_message() -> AgentMessage {
        AgentMessage::Assistant(AssistantMessage {
            content: vec![
                AssistantContent::Text {
                    text: "hello".to_string(),
                },
                AssistantContent::ToolCall {
                    id: "call-1".to_string(),
                    name: "read".to_string(),
                    arguments: serde_json::json!({}),
                },
            ],
            provider: "anthropic".to_string(),
            model: "claude".to_string(),
            usage: Usage {
                input: 1_000,
                output: 200,
                cache_read: 500,
                cache_write: 100,
                total_tokens: 1_800,
                cost: Cost {
                    total: 1.25,
                    ..Default::default()
                },
            },
            stop_reason: StopReason::Stop,
            error_message: None,
            timestamp: Utc::now().timestamp_millis(),
        })
    }

    #[test]
    fn observes_mixed_message_counts_and_usage() {
        let mut summary = SessionMetricsSummary::default();
        summary.observe_message(&AgentMessage::User(UserMessage {
            content: Vec::new(),
            timestamp: Utc::now().timestamp_millis(),
        }));
        summary.observe_message(&assistant_message());
        summary.observe_message(&AgentMessage::ToolResult(ToolResultMessage {
            tool_call_id: "call-1".to_string(),
            tool_name: "read".to_string(),
            content: Vec::new(),
            details: None,
            is_error: false,
            timestamp: Utc::now().timestamp_millis(),
        }));

        assert_eq!(summary.user_messages, 1);
        assert_eq!(summary.assistant_messages, 1);
        assert_eq!(summary.tool_calls, 1);
        assert_eq!(summary.tool_results, 1);
        assert_eq!(summary.total_messages, 3);
        assert_eq!(summary.usage.input_tokens, 1_000);
        assert_eq!(summary.usage.output_tokens, 200);
        assert_eq!(summary.usage.cache_read_tokens, 500);
        assert_eq!(summary.usage.cache_write_tokens, 100);
        assert_eq!(summary.usage.total_tokens, 1_800);
        assert!((summary.usage.total_cost - 1.25).abs() < 1e-9);
    }

    #[test]
    fn collects_metrics_from_session_entries() {
        let entry = SessionEntry::Message {
            base: EntryBase {
                id: EntryId::generate(),
                parent_id: None,
                timestamp: Utc::now(),
            },
            message: assistant_message(),
        };

        let summary = collect_session_metrics([&entry]);
        assert_eq!(summary.assistant_messages, 1);
        assert_eq!(summary.tool_calls, 1);
        assert_eq!(summary.usage.total_tokens, 1_800);
    }
}
