use bb_core::types::{
    AgentMessage, AssistantContent, AssistantMessage, CacheMetricsSource, SessionEntry,
};
use serde::{Deserialize, Serialize};

use crate::usage::UsageTotals;

/// Reusable session-level metrics derived from persisted semantic messages.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionCacheMetricsSource {
    #[default]
    Unknown,
    Official,
    Estimated,
    Mixed,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SessionMetricsSummary {
    pub user_messages: u64,
    pub assistant_messages: u64,
    pub tool_calls: u64,
    pub tool_results: u64,
    pub total_messages: u64,
    pub usage: UsageTotals,
    pub cache_metrics_source: Option<SessionCacheMetricsSource>,
}

impl SessionMetricsSummary {
    pub fn cache_read_hit_rate_pct(&self) -> Option<f64> {
        crate::cache_read_hit_rate_pct(self.usage.input_tokens, self.usage.cache_read_tokens)
    }

    pub fn cache_effective_utilization_pct(&self) -> Option<f64> {
        crate::cache_effective_utilization_pct(
            self.usage.input_tokens,
            self.usage.cache_read_tokens,
            self.usage.cache_write_tokens,
        )
    }

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
        self.usage.total_tokens = self.usage.input_tokens
            + self.usage.output_tokens
            + self.usage.cache_read_tokens
            + self.usage.cache_write_tokens;
        self.merge_cache_metrics_source(message.usage.cache_metrics_source.as_ref());
    }

    fn merge_cache_metrics_source(&mut self, source: Option<&CacheMetricsSource>) {
        let next = match source {
            Some(CacheMetricsSource::Official) => SessionCacheMetricsSource::Official,
            Some(CacheMetricsSource::Estimated) => SessionCacheMetricsSource::Estimated,
            Some(CacheMetricsSource::Unknown) | None => SessionCacheMetricsSource::Unknown,
        };

        match &mut self.cache_metrics_source {
            None => self.cache_metrics_source = Some(next),
            Some(existing) if *existing == next => {}
            Some(SessionCacheMetricsSource::Unknown | SessionCacheMetricsSource::Mixed) => {}
            Some(existing) => {
                *existing = if matches!(next, SessionCacheMetricsSource::Unknown) {
                    SessionCacheMetricsSource::Unknown
                } else {
                    SessionCacheMetricsSource::Mixed
                };
            }
        }
    }
}

pub fn render_cache_metrics_source(source: &SessionCacheMetricsSource) -> &'static str {
    match source {
        SessionCacheMetricsSource::Official => "official",
        SessionCacheMetricsSource::Estimated => "estimated",
        SessionCacheMetricsSource::Mixed => "mixed",
        SessionCacheMetricsSource::Unknown => "unknown",
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
    use super::{
        SessionCacheMetricsSource, SessionMetricsSummary, collect_session_metrics,
        render_cache_metrics_source,
    };
    use bb_core::types::{
        AgentMessage, AssistantContent, AssistantMessage, CacheMetricsSource, ContentBlock, Cost,
        EntryBase, EntryId, SessionEntry, StopReason, ToolResultMessage, Usage, UserMessage,
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
                cache_metrics_source: Some(CacheMetricsSource::Official),
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
        assert_eq!(
            summary.cache_metrics_source,
            Some(SessionCacheMetricsSource::Official)
        );
        assert_eq!(summary.cache_read_hit_rate_pct(), Some(33.333333333333336));
        assert_eq!(summary.cache_effective_utilization_pct(), Some(31.25));
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

    #[test]
    fn merges_cache_source_and_counts_tool_results() {
        let assistant_official = SessionEntry::Message {
            base: EntryBase {
                id: EntryId::generate(),
                parent_id: None,
                timestamp: Utc::now(),
            },
            message: AgentMessage::Assistant(AssistantMessage {
                content: vec![AssistantContent::ToolCall {
                    id: "call-1".to_string(),
                    name: "read".to_string(),
                    arguments: serde_json::json!({}),
                }],
                provider: "anthropic".to_string(),
                model: "claude".to_string(),
                usage: Usage {
                    input: 100,
                    output: 20,
                    cache_read: 40,
                    cache_write: 0,
                    total_tokens: 160,
                    cost: Cost::default(),
                    cache_metrics_source: Some(CacheMetricsSource::Official),
                },
                stop_reason: StopReason::ToolUse,
                error_message: None,
                timestamp: Utc::now().timestamp_millis(),
            }),
        };
        let tool_result = SessionEntry::Message {
            base: EntryBase {
                id: EntryId::generate(),
                parent_id: None,
                timestamp: Utc::now(),
            },
            message: AgentMessage::ToolResult(ToolResultMessage {
                tool_call_id: "call-1".to_string(),
                tool_name: "read".to_string(),
                content: vec![ContentBlock::Text {
                    text: "ok".to_string(),
                }],
                details: Some(serde_json::json!({"ok": true})),
                is_error: false,
                timestamp: Utc::now().timestamp_millis(),
            }),
        };
        let assistant_estimated = SessionEntry::Message {
            base: EntryBase {
                id: EntryId::generate(),
                parent_id: None,
                timestamp: Utc::now(),
            },
            message: AgentMessage::Assistant(AssistantMessage {
                content: Vec::new(),
                provider: "anthropic".to_string(),
                model: "claude".to_string(),
                usage: Usage {
                    input: 60,
                    output: 15,
                    cache_read: 30,
                    cache_write: 0,
                    total_tokens: 105,
                    cost: Cost::default(),
                    cache_metrics_source: Some(CacheMetricsSource::Estimated),
                },
                stop_reason: StopReason::Stop,
                error_message: None,
                timestamp: Utc::now().timestamp_millis(),
            }),
        };

        let summary =
            collect_session_metrics([&assistant_official, &tool_result, &assistant_estimated]);
        assert_eq!(summary.assistant_messages, 2);
        assert_eq!(summary.tool_calls, 1);
        assert_eq!(summary.tool_results, 1);
        assert_eq!(summary.total_messages, 3);
        assert_eq!(summary.usage.input_tokens, 160);
        assert_eq!(summary.usage.output_tokens, 35);
        assert_eq!(summary.usage.cache_read_tokens, 70);
        assert_eq!(summary.usage.total_tokens, 265);
        assert_eq!(
            summary.cache_metrics_source,
            Some(SessionCacheMetricsSource::Mixed)
        );
        assert_eq!(
            render_cache_metrics_source(&SessionCacheMetricsSource::Estimated),
            "estimated"
        );
    }
}
