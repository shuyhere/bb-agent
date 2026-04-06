use anyhow::Result;
use bb_core::settings::Settings;
use bb_core::types::{AgentMessage, AssistantContent, AssistantMessage, SessionEntry, Usage};
use bb_provider::registry::{CostConfig, ModelRegistry};
use bb_session::store;
use rusqlite::Connection;
use std::path::Path;

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct SessionInfoSummary {
    pub file: String,
    pub id: String,
    pub model: String,
    pub thinking: String,
    pub auth_source: String,
    pub copilot_authority: Option<String>,
    pub copilot_login: Option<String>,
    pub copilot_api_base_url: Option<String>,
    pub copilot_cached_model_count: Option<usize>,
    pub copilot_github_access_expires_at: Option<i64>,
    pub copilot_runtime_expires_at: Option<i64>,
    pub user_messages: u64,
    pub assistant_messages: u64,
    pub tool_calls: u64,
    pub tool_results: u64,
    pub total_messages: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub total_tokens: u64,
    pub total_cost: f64,
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

pub(crate) fn collect_session_info_summary(
    conn: &Connection,
    session_id: &str,
    model_provider: &str,
    model_id: &str,
    thinking: &str,
) -> Result<SessionInfoSummary> {
    let file = conn
        .path()
        .map(|p| p.to_string())
        .unwrap_or_else(|| "in-memory".to_string());

    let mut summary = SessionInfoSummary {
        file,
        id: session_id.to_string(),
        model: format!("{model_provider}/{model_id}"),
        thinking: thinking.to_string(),
        auth_source: crate::login::auth_source_label(model_provider).to_string(),
        ..SessionInfoSummary::default()
    };

    if model_provider == "github-copilot" {
        let copilot = crate::login::github_copilot_status();
        summary.copilot_authority = copilot.authority;
        summary.copilot_login = copilot.login;
        summary.copilot_api_base_url = copilot.api_base_url;
        summary.copilot_cached_model_count = Some(copilot.cached_models.len());
        summary.copilot_github_access_expires_at = copilot.github_access_expires_at;
        summary.copilot_runtime_expires_at = copilot.copilot_expires_at;
    }

    let registry = load_session_model_registry(conn, session_id);
    let rows = store::get_entries(conn, session_id)?;
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

    Ok(summary)
}

pub(crate) fn render_session_info_text(summary: &SessionInfoSummary) -> String {
    let mut out = String::from("Session Info\n\n");
    out.push_str(&format!("File: {}\n", summary.file));
    out.push_str(&format!("ID: {}\n", summary.id));
    out.push_str(&format!("Model: {}\n", summary.model));
    out.push_str(&format!("Thinking: {}\n", summary.thinking));
    out.push_str(&format!("Auth: {}\n", summary.auth_source));
    if let Some(authority) = &summary.copilot_authority {
        out.push_str(&format!("Copilot Authority: {}\n", authority));
    }
    if let Some(login) = &summary.copilot_login {
        out.push_str(&format!("Copilot Login: {}\n", login));
    }
    if let Some(api_base_url) = &summary.copilot_api_base_url {
        out.push_str(&format!("Copilot API: {}\n", api_base_url));
    }
    if let Some(count) = summary.copilot_cached_model_count {
        out.push_str(&format!("Copilot Cached Models: {}\n", count));
    }
    if let Some(expires_at) = summary.copilot_github_access_expires_at {
        out.push_str(&format!(
            "GitHub OAuth Expires: {}\n",
            format_timestamp(expires_at)
        ));
    }
    if let Some(expires_at) = summary.copilot_runtime_expires_at {
        out.push_str(&format!(
            "Copilot Runtime Expires: {}\n",
            format_timestamp(expires_at)
        ));
    }
    out.push('\n');

    out.push_str("Messages\n");
    out.push_str(&format!("User: {}\n", format_u64(summary.user_messages)));
    out.push_str(&format!(
        "Assistant: {}\n",
        format_u64(summary.assistant_messages)
    ));
    out.push_str(&format!("Tool Calls: {}\n", format_u64(summary.tool_calls)));
    out.push_str(&format!(
        "Tool Results: {}\n",
        format_u64(summary.tool_results)
    ));
    out.push_str(&format!(
        "Total: {}\n\n",
        format_u64(summary.total_messages)
    ));

    out.push_str("Tokens\n");
    out.push_str(&format!("Input: {}\n", format_u64(summary.input_tokens)));
    out.push_str(&format!("Output: {}\n", format_u64(summary.output_tokens)));
    if summary.cache_read_tokens > 0 {
        out.push_str(&format!(
            "Cache Read: {}\n",
            format_u64(summary.cache_read_tokens)
        ));
    }
    if summary.cache_write_tokens > 0 {
        out.push_str(&format!(
            "Cache Write: {}\n",
            format_u64(summary.cache_write_tokens)
        ));
    }
    out.push_str(&format!("Total: {}\n", format_u64(summary.total_tokens)));

    if summary.total_cost > 0.0 {
        out.push_str("\nCost\n");
        out.push_str(&format!("Total: {:.4}\n", summary.total_cost));
    }

    out
}

fn format_timestamp(timestamp_ms: i64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(timestamp_ms)
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_else(|| timestamp_ms.to_string())
}

fn format_u64(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (idx, ch) in digits.chars().rev().enumerate() {
        if idx > 0 && idx % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out.chars().rev().collect()
}

#[cfg(test)]
mod tests {
    use super::{collect_session_info_summary, format_u64};
    use bb_core::types::{
        AgentMessage, AssistantMessage, Cost, EntryBase, EntryId, SessionEntry, StopReason, Usage,
    };
    use bb_session::store;
    use chrono::Utc;

    #[test]
    fn format_u64_inserts_commas() {
        assert_eq!(format_u64(0), "0");
        assert_eq!(format_u64(12), "12");
        assert_eq!(format_u64(1234), "1,234");
        assert_eq!(format_u64(27064604), "27,064,604");
    }

    #[test]
    fn session_summary_recomputes_cost_from_model_registry() {
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
                },
                stop_reason: StopReason::Stop,
                error_message: None,
                timestamp: Utc::now().timestamp_millis(),
            }),
        };
        store::append_entry(&conn, &session_id, &assistant).expect("append assistant");

        let summary = collect_session_info_summary(
            &conn,
            &session_id,
            "anthropic",
            "claude-opus-4-6",
            "medium",
        )
        .expect("summary");

        assert!((summary.total_cost - 36.75).abs() < 1e-9);
    }
}
