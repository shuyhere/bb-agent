use anyhow::Result;
use bb_monitor::{SessionMetricsSummary, format_u64_with_commas};
use bb_session::store;
use bb_tools::ExecutionPolicy;
use rusqlite::Connection;

/// Aggregated session metadata shown by `/info` and related CLI surfaces.
///
/// Copilot-specific fields are only populated when the active provider is
/// `github-copilot`; other providers leave those fields empty.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct SessionInfoSummary {
    pub file: String,
    pub id: String,
    pub model: String,
    pub thinking: String,
    pub execution_mode: ExecutionPolicy,
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

/// Build a reusable summary for session-info rendering.
///
/// The summary intentionally folds together persisted session metrics and the
/// current auth snapshot so callers can render one coherent view without
/// duplicating login/session-info rules.
pub(crate) fn collect_session_info_summary(
    conn: &Connection,
    session_id: &str,
    model_provider: &str,
    model_id: &str,
    thinking: &str,
    execution_policy: ExecutionPolicy,
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
        execution_mode: execution_policy,
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

    let mut metrics = SessionMetricsSummary::default();
    let rows = store::get_entries(conn, session_id)?;
    for row in rows {
        let Ok(entry) = store::parse_entry(&row) else {
            continue;
        };
        metrics.observe_entry(&entry);
    }

    summary.user_messages = metrics.user_messages;
    summary.assistant_messages = metrics.assistant_messages;
    summary.tool_calls = metrics.tool_calls;
    summary.tool_results = metrics.tool_results;
    summary.total_messages = metrics.total_messages;
    summary.input_tokens = metrics.usage.input_tokens;
    summary.output_tokens = metrics.usage.output_tokens;
    summary.cache_read_tokens = metrics.usage.cache_read_tokens;
    summary.cache_write_tokens = metrics.usage.cache_write_tokens;
    summary.total_tokens = metrics.usage.effective_total_tokens();
    summary.total_cost = metrics.usage.total_cost;

    Ok(summary)
}

/// Render the human-readable `/info` text block from a collected summary.
pub(crate) fn render_session_info_text(summary: &SessionInfoSummary) -> String {
    let mut out = String::from("Session Info\n\n");
    out.push_str(&format!("File: {}\n", summary.file));
    out.push_str(&format!("ID: {}\n", summary.id));
    out.push_str(&format!("Model: {}\n", summary.model));
    out.push_str(&format!("Thinking: {}\n", summary.thinking));
    out.push_str(&format!(
        "Permissions: {}\n",
        permission_posture_detail(summary.execution_mode)
    ));
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
    out.push_str(&format!(
        "User: {}\n",
        format_u64_with_commas(summary.user_messages)
    ));
    out.push_str(&format!(
        "Assistant: {}\n",
        format_u64_with_commas(summary.assistant_messages)
    ));
    out.push_str(&format!(
        "Tool Calls: {}\n",
        format_u64_with_commas(summary.tool_calls)
    ));
    out.push_str(&format!(
        "Tool Results: {}\n",
        format_u64_with_commas(summary.tool_results)
    ));
    out.push_str(&format!(
        "Total: {}\n\n",
        format_u64_with_commas(summary.total_messages)
    ));

    out.push_str("Tokens\n");
    out.push_str(&format!(
        "Input: {}\n",
        format_u64_with_commas(summary.input_tokens)
    ));
    out.push_str(&format!(
        "Output: {}\n",
        format_u64_with_commas(summary.output_tokens)
    ));
    if summary.cache_read_tokens > 0 {
        out.push_str(&format!(
            "Cache Read: {}\n",
            format_u64_with_commas(summary.cache_read_tokens)
        ));
    }
    if summary.cache_write_tokens > 0 {
        out.push_str(&format!(
            "Cache Write: {}\n",
            format_u64_with_commas(summary.cache_write_tokens)
        ));
    }
    out.push_str(&format!(
        "Total: {}\n",
        format_u64_with_commas(summary.total_tokens)
    ));

    if summary.total_cost > 0.0 {
        out.push_str("\nCost\n");
        out.push_str(&format!("Total: {:.4}\n", summary.total_cost));
    }

    out
}

pub(crate) fn permission_posture_badge(execution_mode: ExecutionPolicy) -> &'static str {
    match execution_mode {
        ExecutionPolicy::Safety => "mode safety/project-only",
        ExecutionPolicy::Yolo => "mode yolo/full-access",
    }
}

pub(crate) fn permission_posture_detail(execution_mode: ExecutionPolicy) -> String {
    format!(
        "{} ({})",
        execution_mode.as_str(),
        execution_mode.write_scope_label()
    )
}

fn format_timestamp(timestamp_ms: i64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(timestamp_ms)
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_else(|| timestamp_ms.to_string())
}

#[cfg(test)]
mod tests {
    use super::{
        SessionInfoSummary, collect_session_info_summary, permission_posture_badge,
        render_session_info_text,
    };
    use bb_core::types::{
        AgentMessage, AssistantMessage, Cost, EntryBase, EntryId, SessionEntry, StopReason, Usage,
    };
    use bb_monitor::format_u64_with_commas;
    use bb_session::store;
    use bb_tools::ExecutionPolicy;
    use chrono::Utc;

    #[test]
    fn format_u64_inserts_commas() {
        assert_eq!(format_u64_with_commas(0), "0");
        assert_eq!(format_u64_with_commas(12), "12");
        assert_eq!(format_u64_with_commas(1234), "1,234");
        assert_eq!(format_u64_with_commas(27064604), "27,064,604");
    }

    #[test]
    fn session_summary_uses_stored_message_cost() {
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
                    cache_metrics_source: None,
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
            ExecutionPolicy::Safety,
        )
        .expect("summary");

        assert!((summary.total_cost - 110.25).abs() < 1e-9);
        assert_eq!(summary.execution_mode, ExecutionPolicy::Safety);
    }

    #[test]
    fn session_summary_renders_permission_posture() {
        let summary = SessionInfoSummary {
            execution_mode: ExecutionPolicy::Yolo,
            ..SessionInfoSummary::default()
        };

        let rendered = render_session_info_text(&summary);
        assert!(rendered.contains("Permissions: yolo (full access)"));
        assert_eq!(
            permission_posture_badge(ExecutionPolicy::Safety),
            "mode safety/project-only"
        );
    }
}
