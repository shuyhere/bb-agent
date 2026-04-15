use anyhow::Result;
use bb_core::settings::Settings;
use bb_core::types::{
    AgentMessage, AssistantContent, AssistantMessage, CacheMetricsSource, SessionEntry, Usage,
};
use bb_provider::registry::{CostConfig, ModelRegistry};
use bb_session::store;
use rusqlite::Connection;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionCacheMetricsSource {
    Official,
    Estimated,
    Mixed,
    Unknown,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SessionMetricsSummary {
    pub user_messages: u64,
    pub assistant_messages: u64,
    pub tool_calls: u64,
    pub tool_results: u64,
    pub total_messages: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub cache_metrics_source: Option<SessionCacheMetricsSource>,
    pub cache_read_hit_rate_pct: Option<f64>,
    pub cache_effective_utilization_pct: Option<f64>,
    pub total_tokens: u64,
    pub total_cost: f64,
}

pub fn render_cache_metrics_source(source: &SessionCacheMetricsSource) -> &'static str {
    match source {
        SessionCacheMetricsSource::Official => "official",
        SessionCacheMetricsSource::Estimated => "estimated",
        SessionCacheMetricsSource::Mixed => "mixed",
        SessionCacheMetricsSource::Unknown => "unknown",
    }
}

pub fn collect_session_metrics(
    conn: &Connection,
    session_id: &str,
) -> Result<SessionMetricsSummary> {
    let registry = load_session_model_registry(conn, session_id);
    let rows = store::get_entries(conn, session_id)?;
    let mut summary = SessionMetricsSummary::default();

    for row in rows {
        let Ok(entry) = store::parse_entry(&row) else {
            continue;
        };
        if let SessionEntry::Message { message, .. } = entry {
            summary.total_messages += 1;
            match message {
                AgentMessage::User(_) => summary.user_messages += 1,
                AgentMessage::Assistant(message) => {
                    summary.assistant_messages += 1;
                    summary.tool_calls += message
                        .content
                        .iter()
                        .filter(|content| matches!(content, AssistantContent::ToolCall { .. }))
                        .count() as u64;
                    summary.input_tokens += message.usage.input;
                    summary.output_tokens += message.usage.output;
                    summary.cache_read_tokens += message.usage.cache_read;
                    summary.cache_write_tokens += message.usage.cache_write;
                    merge_cache_metrics_source(
                        &mut summary.cache_metrics_source,
                        message.usage.cache_metrics_source.as_ref(),
                    );
                    summary.total_cost += recompute_assistant_message_cost(&message, &registry);
                }
                AgentMessage::ToolResult(_) => summary.tool_results += 1,
                _ => {}
            }
        }
    }

    summary.total_tokens = summary.input_tokens
        + summary.output_tokens
        + summary.cache_read_tokens
        + summary.cache_write_tokens;
    summary.cache_read_hit_rate_pct =
        crate::cache_read_hit_rate_pct(summary.input_tokens, summary.cache_read_tokens);
    summary.cache_effective_utilization_pct = crate::cache_effective_utilization_pct(
        summary.input_tokens,
        summary.cache_read_tokens,
        summary.cache_write_tokens,
    );

    Ok(summary)
}

fn cache_metrics_source_state(source: Option<&CacheMetricsSource>) -> SessionCacheMetricsSource {
    match source {
        Some(CacheMetricsSource::Official) => SessionCacheMetricsSource::Official,
        Some(CacheMetricsSource::Estimated) => SessionCacheMetricsSource::Estimated,
        Some(CacheMetricsSource::Unknown) | None => SessionCacheMetricsSource::Unknown,
    }
}

fn merge_cache_metrics_source(
    current: &mut Option<SessionCacheMetricsSource>,
    source: Option<&CacheMetricsSource>,
) {
    let next = cache_metrics_source_state(source);
    match current {
        None => *current = Some(next),
        Some(existing) if *existing == next => {}
        Some(SessionCacheMetricsSource::Unknown) | Some(SessionCacheMetricsSource::Mixed) => {}
        Some(existing) => {
            *existing = if matches!(next, SessionCacheMetricsSource::Unknown) {
                SessionCacheMetricsSource::Unknown
            } else {
                SessionCacheMetricsSource::Mixed
            };
        }
    }
}

fn load_session_model_registry(conn: &Connection, session_id: &str) -> ModelRegistry {
    let mut registry = ModelRegistry::new();
    if let Ok(Some(row)) = store::get_session(conn, session_id) {
        registry.load_custom_models(&Settings::load_merged(Path::new(&row.cwd)));
    }
    registry
}

fn calculate_usage_total_cost(usage: &Usage, model_cost: &CostConfig) -> f64 {
    (model_cost.input / 1_000_000.0) * usage.input as f64
        + (model_cost.output / 1_000_000.0) * usage.output as f64
        + (model_cost.cache_read / 1_000_000.0) * usage.cache_read as f64
        + (model_cost.cache_write / 1_000_000.0) * usage.cache_write as f64
}

fn recompute_assistant_message_cost(message: &AssistantMessage, registry: &ModelRegistry) -> f64 {
    registry
        .find(&message.provider, &message.model)
        .or_else(|| registry.find_fuzzy(&message.model, Some(&message.provider)))
        .or_else(|| registry.find_fuzzy(&message.model, None))
        .map(|model| calculate_usage_total_cost(&message.usage, &model.cost))
        .unwrap_or(message.usage.cost.total)
}

#[cfg(test)]
mod tests {
    use super::{SessionCacheMetricsSource, collect_session_metrics, render_cache_metrics_source};
    use bb_core::types::{
        AgentMessage, AssistantContent, AssistantMessage, CacheMetricsSource, ContentBlock, Cost,
        EntryBase, EntryId, SessionEntry, StopReason, ToolResultMessage, Usage,
    };
    use bb_session::store;
    use chrono::Utc;
    use serde_json::json;

    #[test]
    fn session_metrics_recompute_cost_from_model_registry() {
        let conn = store::open_memory().expect("memory db");
        let session_id = store::create_session(&conn, "/tmp").expect("session");
        let assistant = SessionEntry::Message {
            base: EntryBase {
                id: EntryId::generate(),
                parent_id: None,
                timestamp: Utc::now(),
            },
            message: AgentMessage::Assistant(AssistantMessage {
                content: Vec::new(),
                provider: "anthropic".to_string(),
                model: "claude-opus-4-6".to_string(),
                usage: Usage {
                    input: 1_000_000,
                    output: 1_000_000,
                    cache_read: 1_000_000,
                    cache_write: 1_000_000,
                    total_tokens: 4_000_000,
                    cost: Cost {
                        input: 15.0,
                        output: 75.0,
                        cache_read: 1.5,
                        cache_write: 18.75,
                        total: 110.25,
                    },
                    cache_metrics_source: Some(CacheMetricsSource::Official),
                },
                stop_reason: StopReason::Stop,
                error_message: None,
                timestamp: Utc::now().timestamp_millis(),
            }),
        };
        store::append_entry(&conn, &session_id, &assistant).expect("append assistant");

        let summary = collect_session_metrics(&conn, &session_id).expect("summary");

        assert!((summary.total_cost - 36.75).abs() < 1e-9);
        assert_eq!(summary.tool_calls, 0);
        assert_eq!(summary.tool_results, 0);
        assert_eq!(
            summary.cache_metrics_source,
            Some(SessionCacheMetricsSource::Official)
        );
        assert_eq!(summary.cache_read_hit_rate_pct, Some(50.0));
        assert!(
            (summary.cache_effective_utilization_pct.unwrap_or_default() - 33.333333333333336)
                .abs()
                < 1e-9
        );
    }

    #[test]
    fn session_metrics_count_tool_usage_and_merge_cache_sources() {
        let conn = store::open_memory().expect("memory db");
        let session_id = store::create_session(&conn, "/tmp").expect("session");

        let assistant_with_tool_call = SessionEntry::Message {
            base: EntryBase {
                id: EntryId::generate(),
                parent_id: None,
                timestamp: Utc::now(),
            },
            message: AgentMessage::Assistant(AssistantMessage {
                content: vec![AssistantContent::ToolCall {
                    id: "call_1".to_string(),
                    name: "web_fetch".to_string(),
                    arguments: json!({"url": "https://example.com"}),
                }],
                provider: "anthropic".to_string(),
                model: "claude-opus-4-6".to_string(),
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
        store::append_entry(&conn, &session_id, &assistant_with_tool_call)
            .expect("append assistant with tool call");

        let tool_result = SessionEntry::Message {
            base: EntryBase {
                id: EntryId::generate(),
                parent_id: None,
                timestamp: Utc::now(),
            },
            message: AgentMessage::ToolResult(ToolResultMessage {
                tool_call_id: "call_1".to_string(),
                tool_name: "web_fetch".to_string(),
                content: vec![ContentBlock::Text {
                    text: "ok".to_string(),
                }],
                details: Some(json!({"ok": true})),
                is_error: false,
                timestamp: Utc::now().timestamp_millis(),
            }),
        };
        store::append_entry(&conn, &session_id, &tool_result).expect("append tool result");

        let assistant_estimated = SessionEntry::Message {
            base: EntryBase {
                id: EntryId::generate(),
                parent_id: None,
                timestamp: Utc::now(),
            },
            message: AgentMessage::Assistant(AssistantMessage {
                content: Vec::new(),
                provider: "anthropic".to_string(),
                model: "claude-opus-4-6".to_string(),
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
        store::append_entry(&conn, &session_id, &assistant_estimated)
            .expect("append estimated assistant");

        let summary = collect_session_metrics(&conn, &session_id).expect("summary");

        assert_eq!(summary.assistant_messages, 2);
        assert_eq!(summary.tool_calls, 1);
        assert_eq!(summary.tool_results, 1);
        assert_eq!(summary.total_messages, 3);
        assert_eq!(summary.input_tokens, 160);
        assert_eq!(summary.output_tokens, 35);
        assert_eq!(summary.cache_read_tokens, 70);
        assert_eq!(summary.total_tokens, 265);
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
