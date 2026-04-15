use anyhow::Result;
use bb_core::types::*;
use bb_provider::CollectedResponse;

use crate::cache_metrics::ResolvedCacheUsage;
use bb_provider::registry::Model;
use bb_session::store;
use chrono::Utc;
use std::sync::Arc;
use tokio::sync::Mutex;

fn next_entry_base(conn: &rusqlite::Connection, session_id: &str) -> EntryBase {
    EntryBase {
        id: EntryId::generate(),
        parent_id: get_leaf_raw(conn, session_id),
        timestamp: Utc::now(),
    }
}

fn assistant_content_from_response(collected: &CollectedResponse) -> Vec<AssistantContent> {
    let mut assistant_content = Vec::new();
    if !collected.thinking.is_empty() {
        assistant_content.push(AssistantContent::Thinking {
            thinking: collected.thinking.clone(),
        });
    }
    if !collected.text.is_empty() {
        assistant_content.push(AssistantContent::Text {
            text: collected.text.clone(),
        });
    }
    for tool_call in &collected.tool_calls {
        let arguments = serde_json::from_str(&tool_call.arguments).unwrap_or(serde_json::json!({}));
        assistant_content.push(AssistantContent::ToolCall {
            id: tool_call.id.clone(),
            name: tool_call.name.clone(),
            arguments,
        });
    }
    assistant_content
}

fn calculate_cost(model: &Model, usage: &ResolvedCacheUsage) -> Cost {
    let inp = usage.effective_input_tokens;
    let out = usage.effective_output_tokens;
    let cr = usage.effective_cache_read_tokens;
    let cw = usage.effective_cache_write_tokens;
    let model_cost = &model.cost;

    Cost {
        input: (model_cost.input / 1_000_000.0) * inp as f64,
        output: (model_cost.output / 1_000_000.0) * out as f64,
        cache_read: (model_cost.cache_read / 1_000_000.0) * cr as f64,
        cache_write: (model_cost.cache_write / 1_000_000.0) * cw as f64,
        total: (model_cost.input / 1_000_000.0) * inp as f64
            + (model_cost.output / 1_000_000.0) * out as f64
            + (model_cost.cache_read / 1_000_000.0) * cr as f64
            + (model_cost.cache_write / 1_000_000.0) * cw as f64,
    }
}

pub(crate) async fn append_user_message_with_images(
    conn: &Arc<Mutex<rusqlite::Connection>>,
    session_id: &str,
    prompt: &str,
    images: &[bb_core::agent_session::ImageContent],
) -> Result<()> {
    let conn = conn.lock().await;
    let mut content = vec![ContentBlock::Text {
        text: prompt.to_string(),
    }];
    content.extend(images.iter().map(|image| {
        ContentBlock::Image {
            data: image.source.clone(),
            mime_type: image
                .mime_type
                .clone()
                .unwrap_or_else(|| "image/png".to_string()),
        }
    }));
    let user_entry = SessionEntry::Message {
        base: next_entry_base(&conn, session_id),
        message: AgentMessage::User(UserMessage {
            content,
            timestamp: Utc::now().timestamp_millis(),
        }),
    };
    store::append_entry(&conn, session_id, &user_entry)?;
    Ok(())
}

pub(super) async fn append_custom_message(
    conn: &Arc<Mutex<rusqlite::Connection>>,
    session_id: &str,
    message: serde_json::Value,
) -> Result<()> {
    let custom_message: CustomMessage = serde_json::from_value(message)?;
    let conn = conn.lock().await;
    let custom_entry = SessionEntry::CustomMessage {
        base: next_entry_base(&conn, session_id),
        custom_type: custom_message.custom_type,
        content: custom_message.content,
        display: custom_message.display,
        details: custom_message.details,
    };
    store::append_entry(&conn, session_id, &custom_entry)?;
    Ok(())
}

pub(super) fn append_assistant_message(
    conn: &rusqlite::Connection,
    session_id: &str,
    model: &Model,
    collected: &CollectedResponse,
    usage: &ResolvedCacheUsage,
) -> Result<()> {
    let inp = usage.effective_input_tokens;
    let out = usage.effective_output_tokens;
    let cr = usage.effective_cache_read_tokens;
    let cw = usage.effective_cache_write_tokens;

    let assistant_entry = SessionEntry::Message {
        base: next_entry_base(conn, session_id),
        message: AgentMessage::Assistant(AssistantMessage {
            content: assistant_content_from_response(collected),
            provider: model.provider.clone(),
            model: model.id.clone(),
            usage: Usage {
                input: inp,
                output: out,
                cache_read: cr,
                cache_write: cw,
                total_tokens: usage.prompt_token_total + out,
                cost: calculate_cost(model, usage),
                cache_metrics_source: Some(usage.cache_metrics_source.clone()),
            },
            stop_reason: if collected.tool_calls.is_empty() {
                StopReason::Stop
            } else {
                StopReason::ToolUse
            },
            error_message: None,
            timestamp: Utc::now().timestamp_millis(),
        }),
    };
    store::append_entry(conn, session_id, &assistant_entry)?;
    Ok(())
}

pub(crate) fn get_leaf_raw(conn: &rusqlite::Connection, session_id: &str) -> Option<EntryId> {
    store::get_session(conn, session_id)
        .ok()
        .flatten()
        .and_then(|session| session.leaf_id.map(EntryId))
}

pub(crate) fn open_sibling_conn(
    conn: &rusqlite::Connection,
) -> Result<Arc<Mutex<rusqlite::Connection>>> {
    let path = conn.path().map(std::path::PathBuf::from);
    let new_conn = match path {
        Some(path) => store::open_db(&path)?,
        None => store::open_memory()?,
    };
    Ok(Arc::new(Mutex::new(new_conn)))
}

pub(crate) fn wrap_conn(conn: rusqlite::Connection) -> Arc<Mutex<rusqlite::Connection>> {
    Arc::new(Mutex::new(conn))
}

#[cfg(test)]
mod tests {
    use super::calculate_cost;
    use crate::cache_metrics::ResolvedCacheUsage;
    use bb_core::types::CacheMetricsSource;
    use bb_provider::registry::{ApiType, CostConfig, Model, ModelInput};

    fn test_model() -> Model {
        Model {
            id: "claude-sonnet-4-6".to_string(),
            name: "Claude Sonnet 4.6".to_string(),
            provider: "anthropic".to_string(),
            api: ApiType::AnthropicMessages,
            context_window: 200_000,
            max_tokens: 8_192,
            reasoning: true,
            input: vec![ModelInput::Text],
            base_url: Some("https://api.anthropic.com".to_string()),
            cost: CostConfig {
                input: 3.0,
                output: 15.0,
                cache_read: 0.3,
                cache_write: 3.75,
            },
        }
    }

    #[test]
    fn estimated_cost_stays_close_to_official_when_cache_estimate_is_close() {
        let model = test_model();
        let official = ResolvedCacheUsage {
            cache_metrics_source: CacheMetricsSource::Official,
            effective_input_tokens: 180,
            effective_output_tokens: 48,
            effective_cache_read_tokens: 320,
            effective_cache_write_tokens: 0,
            prompt_token_total: 500,
            provider_cache_read_tokens: Some(320),
            provider_cache_write_tokens: Some(0),
            estimated_cache_read_tokens: None,
            estimated_cache_write_tokens: None,
            warm_request: true,
        };
        let estimated = ResolvedCacheUsage {
            cache_metrics_source: CacheMetricsSource::Estimated,
            effective_input_tokens: 186,
            effective_output_tokens: 48,
            effective_cache_read_tokens: 314,
            effective_cache_write_tokens: 0,
            prompt_token_total: 500,
            provider_cache_read_tokens: Some(320),
            provider_cache_write_tokens: Some(0),
            estimated_cache_read_tokens: Some(314),
            estimated_cache_write_tokens: Some(0),
            warm_request: true,
        };

        let official_cost = calculate_cost(&model, &official);
        let estimated_cost = calculate_cost(&model, &estimated);
        let total_delta = (estimated_cost.total - official_cost.total).abs();
        let official_total = official_cost.total.max(f64::EPSILON);
        let total_delta_pct = (total_delta / official_total) * 100.0;

        assert!(
            total_delta_pct <= 2.0,
            "cost delta {total_delta_pct:.3}% exceeded tolerance (official={}, estimated={})",
            official_cost.total,
            estimated_cost.total,
        );
    }
}
