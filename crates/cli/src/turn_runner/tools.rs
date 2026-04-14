use anyhow::Result;
use bb_core::tool_names::normalize_requested_tool_name;
use bb_core::types::*;
use bb_hooks::events::ToolResultEvent;
use bb_hooks::{Event, ToolCallEvent};
use bb_provider::{CollectedResponse, CollectedToolCall};
use bb_session::store;
use bb_tools::{Tool, ToolContext};
use chrono::Utc;
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};
use tokio_util::sync::CancellationToken;

use super::TurnEvent;
use super::hooks::send_extension_event_safe;
use super::panic::catch_contained_panics;
use super::persistence::get_leaf_raw;
use crate::extensions::ExtensionCommandRegistry;

pub(super) struct ToolExecutionEnv<'a> {
    pub conn: &'a Arc<Mutex<rusqlite::Connection>>,
    pub session_id: &'a str,
    pub tools: &'a [Box<dyn Tool>],
    pub tool_ctx: &'a ToolContext,
    pub cancel: &'a CancellationToken,
    pub extensions: &'a ExtensionCommandRegistry,
    pub event_tx: &'a mpsc::UnboundedSender<TurnEvent>,
}

pub(super) async fn execute_tool_calls(
    collected: &CollectedResponse,
    env: ToolExecutionEnv<'_>,
) -> Result<()> {
    for tool_call in &collected.tool_calls {
        let mut args = serde_json::from_str(&tool_call.arguments).unwrap_or(serde_json::json!({}));

        let _ = env.event_tx.send(TurnEvent::ToolExecuting {
            id: tool_call.id.clone(),
        });

        let hook_result = send_extension_event_safe(
            env.extensions,
            Event::ToolCall(ToolCallEvent::new(
                tool_call.id.clone(),
                tool_call.name.clone(),
                args.clone(),
            )),
            env.event_tx,
            "ToolCall",
        )
        .await;

        if let Some(updated_args) = hook_result.as_ref().and_then(|result| result.input.clone()) {
            args = updated_args;
        }

        let started_at = std::time::Instant::now();
        let result = if hook_result.as_ref().and_then(|result| result.block) == Some(true) {
            Err(bb_core::error::BbError::Tool(
                hook_result
                    .and_then(|result| result.reason)
                    .unwrap_or_else(|| format!("Tool {} blocked by extension", tool_call.name)),
            ))
        } else {
            execute_tool(tool_call, args.clone(), &env).await
        };
        let duration_ms = started_at.elapsed().as_millis() as u64;

        let (mut content, mut details, artifact_path, mut is_error) = match result {
            Ok(result) => (
                result.content,
                result.details,
                result.artifact_path.map(|path| path.display().to_string()),
                result.is_error,
            ),
            Err(error) => (
                vec![ContentBlock::Text {
                    text: format!("Error: {error}"),
                }],
                None,
                None,
                true,
            ),
        };
        let mut details_json = details
            .take()
            .unwrap_or_else(|| serde_json::Value::Object(serde_json::Map::new()));
        if !details_json.is_object() {
            details_json = serde_json::json!({ "value": details_json });
        }
        if let Some(map) = details_json.as_object_mut() {
            map.insert(
                "durationMs".to_string(),
                serde_json::Value::from(duration_ms),
            );
        }
        details = Some(details_json);

        if let Some(result) = send_extension_event_safe(
            env.extensions,
            Event::ToolResult(ToolResultEvent::new(
                tool_call.id.clone(),
                tool_call.name.clone(),
                args.clone(),
                content.clone(),
                details.clone(),
                is_error,
            )),
            env.event_tx,
            "ToolResult",
        )
        .await
        {
            if let Some(updated_content) = result.content {
                content = updated_content
                    .into_iter()
                    .filter_map(|block| serde_json::from_value::<ContentBlock>(block).ok())
                    .collect();
            }
            if let Some(updated_details) = result.details {
                details = Some(updated_details);
            }
            if let Some(updated_is_error) = result.is_error {
                is_error = updated_is_error;
            }
        }

        persist_tool_result(&env, tool_call, content, details, artifact_path, is_error).await?;
    }

    Ok(())
}

pub(super) async fn append_cancelled_tool_results(
    collected: &CollectedResponse,
    env: ToolExecutionEnv<'_>,
) -> Result<()> {
    for tool_call in &collected.tool_calls {
        persist_tool_result(
            &env,
            tool_call,
            vec![ContentBlock::Text {
                text: "Error: tool execution cancelled before start".to_string(),
            }],
            Some(serde_json::json!({
                "cancelled": true,
                "durationMs": 0,
            })),
            None,
            true,
        )
        .await?;
    }

    Ok(())
}

async fn persist_tool_result(
    env: &ToolExecutionEnv<'_>,
    tool_call: &CollectedToolCall,
    content: Vec<ContentBlock>,
    details: Option<serde_json::Value>,
    artifact_path: Option<String>,
    is_error: bool,
) -> Result<()> {
    let _ = env.event_tx.send(TurnEvent::ToolResult {
        id: tool_call.id.clone(),
        name: tool_call.name.clone(),
        content: content.clone(),
        details: details.clone(),
        artifact_path: artifact_path.clone(),
        is_error,
    });

    let conn = env.conn.lock().await;
    let tool_result_entry = SessionEntry::Message {
        base: EntryBase {
            id: EntryId::generate(),
            parent_id: get_leaf_raw(&conn, env.session_id),
            timestamp: Utc::now(),
        },
        message: AgentMessage::ToolResult(ToolResultMessage {
            tool_call_id: tool_call.id.clone(),
            tool_name: tool_call.name.clone(),
            content,
            details,
            is_error,
            timestamp: Utc::now().timestamp_millis(),
        }),
    };
    store::append_entry(&conn, env.session_id, &tool_result_entry)?;
    Ok(())
}

fn tool_context_with_output_forwarding(
    env: &ToolExecutionEnv<'_>,
    tool_call_id: String,
) -> ToolContext {
    let event_tx = env.event_tx.clone();
    ToolContext {
        cwd: env.tool_ctx.cwd.clone(),
        artifacts_dir: env.tool_ctx.artifacts_dir.clone(),
        execution_policy: env.tool_ctx.execution_policy,
        on_output: Some(Box::new(move |chunk| {
            let _ = event_tx.send(TurnEvent::ToolOutputDelta {
                id: tool_call_id.clone(),
                chunk: chunk.to_string(),
            });
        })),
        web_search: env.tool_ctx.web_search.clone(),
        execution_mode: env.tool_ctx.execution_mode,
        request_approval: env.tool_ctx.request_approval.clone(),
    }
}

async fn execute_tool(
    tool_call: &bb_provider::CollectedToolCall,
    args: serde_json::Value,
    env: &ToolExecutionEnv<'_>,
) -> bb_core::error::BbResult<bb_tools::ToolResult> {
    let normalized_name = normalize_requested_tool_name(&tool_call.name);
    let Some(tool) = env
        .tools
        .iter()
        .find(|tool| tool.name() == normalized_name.as_ref())
    else {
        return Err(bb_core::error::BbError::Tool(format!(
            "Unknown tool: {}",
            tool_call.name
        )));
    };

    let tool_ctx = tool_context_with_output_forwarding(env, tool_call.id.clone());

    match catch_contained_panics(tool.execute(args, &tool_ctx, env.cancel.clone())).await {
        Ok(result) => result,
        Err(message) => Err(bb_core::error::BbError::Tool(format!(
            "Tool {} panicked: {message}",
            tool_call.name
        ))),
    }
}
