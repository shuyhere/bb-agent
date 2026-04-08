use anyhow::Result;
use bb_core::types::*;
use bb_provider::CollectedResponse;
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

fn calculate_cost(model: &Model, collected: &CollectedResponse) -> Cost {
    let inp = collected.input_tokens;
    let out = collected.output_tokens;
    let cr = collected.cache_read_tokens;
    let cw = collected.cache_write_tokens;
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
) -> Result<()> {
    let inp = collected.input_tokens;
    let out = collected.output_tokens;
    let cr = collected.cache_read_tokens;
    let cw = collected.cache_write_tokens;

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
                total_tokens: inp + out + cr + cw,
                cost: calculate_cost(model, collected),
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
