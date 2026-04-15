use anyhow::Result;
use bb_core::tool_names::normalize_requested_tool_name;
use bb_core::types::*;
use bb_hooks::events::{
    ToolExecutionEndEvent, ToolExecutionStartEvent, ToolExecutionUpdateEvent, ToolResultEvent,
};
use bb_hooks::{Event, ToolCallEvent};
use bb_provider::{CollectedResponse, CollectedToolCall};
use bb_session::store;
use bb_tools::{FileQueue, Tool, ToolContext, ToolScheduling, execute_reserved_tool_call};
use chrono::Utc;
use futures::future::join_all;
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
    let file_queue = FileQueue::new();
    let mut pending = Vec::new();

    for (source_index, tool_call) in collected.tool_calls.iter().enumerate() {
        if let Some(prepared) = preflight_tool_call(source_index, tool_call, &env).await? {
            pending.push(execute_prepared_tool_call(prepared, &env, &file_queue));
        }
    }

    let mut first_error = None;
    let mut completed = Vec::new();
    for result in join_all(pending).await {
        match result {
            Ok(executed) => completed.push(executed),
            Err(err) if first_error.is_none() => first_error = Some(err),
            Err(_) => {}
        }
    }

    completed.sort_by_key(|executed| executed.prepared.source_index);
    for executed in completed {
        if let Err(err) = finish_tool_call(executed, &env).await
            && first_error.is_none()
        {
            first_error = Some(err);
        }
    }

    match first_error {
        Some(err) => Err(err),
        None => Ok(()),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ToolExecutionPhase {
    Created,
    ExecutionStarted,
    PreflightComplete,
    Running,
    ResultAvailable,
    ResultHooksApplied,
    ExecutionEnded,
    Persisted,
}

struct ToolExecutionStateMachine {
    phase: ToolExecutionPhase,
}

struct PreparedToolExecution {
    source_index: usize,
    id: String,
    name: String,
    args: serde_json::Value,
    lifecycle: ToolExecutionStateMachine,
}

struct ExecutedToolCall {
    prepared: PreparedToolExecution,
    result: bb_core::error::BbResult<bb_tools::ToolResult>,
    duration_ms: u64,
    ran_execution: bool,
}

impl ToolExecutionStateMachine {
    fn new() -> Self {
        Self {
            phase: ToolExecutionPhase::Created,
        }
    }

    fn transition(&mut self, expected: ToolExecutionPhase, next: ToolExecutionPhase) {
        debug_assert_eq!(
            self.phase, expected,
            "invalid tool execution transition: expected {:?}, got {:?}",
            expected, self.phase
        );
        self.phase = next;
    }
}

async fn preflight_tool_call(
    source_index: usize,
    tool_call: &CollectedToolCall,
    env: &ToolExecutionEnv<'_>,
) -> Result<Option<PreparedToolExecution>> {
    let mut lifecycle = ToolExecutionStateMachine::new();
    let mut args = serde_json::from_str(&tool_call.arguments).unwrap_or(serde_json::json!({}));

    let _ = send_extension_event_safe(
        env.extensions,
        Event::ToolExecutionStart(ToolExecutionStartEvent::new(
            tool_call.id.clone(),
            tool_call.name.clone(),
            args.clone(),
        )),
        env.event_tx,
        "ToolExecutionStart",
    )
    .await;
    lifecycle.transition(
        ToolExecutionPhase::Created,
        ToolExecutionPhase::ExecutionStarted,
    );

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
    lifecycle.transition(
        ToolExecutionPhase::ExecutionStarted,
        ToolExecutionPhase::PreflightComplete,
    );

    let block_requested = hook_result.as_ref().and_then(|result| result.block) == Some(true);
    let block_reason = hook_result
        .as_ref()
        .and_then(|result| result.reason.clone());

    let prepared = PreparedToolExecution {
        source_index,
        id: tool_call.id.clone(),
        name: tool_call.name.clone(),
        args,
        lifecycle,
    };

    if block_requested {
        finish_tool_call(
            ExecutedToolCall {
                prepared,
                result: Err(bb_core::error::BbError::Tool(block_reason.unwrap_or_else(
                    || format!("Tool {} blocked by extension", tool_call.name),
                ))),
                duration_ms: 0,
                ran_execution: false,
            },
            env,
        )
        .await?;
        Ok(None)
    } else {
        Ok(Some(prepared))
    }
}

async fn execute_prepared_tool_call(
    mut prepared: PreparedToolExecution,
    env: &ToolExecutionEnv<'_>,
    file_queue: &FileQueue,
) -> Result<ExecutedToolCall> {
    let started_at = std::time::Instant::now();
    let result = execute_tool(&mut prepared, env, file_queue).await;
    let duration_ms = started_at.elapsed().as_millis() as u64;

    Ok(ExecutedToolCall {
        prepared,
        result,
        duration_ms,
        ran_execution: true,
    })
}

async fn finish_tool_call(executed: ExecutedToolCall, env: &ToolExecutionEnv<'_>) -> Result<()> {
    let ExecutedToolCall {
        mut prepared,
        result,
        duration_ms,
        ran_execution,
    } = executed;
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
    if ran_execution {
        prepared.lifecycle.transition(
            ToolExecutionPhase::Running,
            ToolExecutionPhase::ResultAvailable,
        );
    } else {
        prepared.lifecycle.transition(
            ToolExecutionPhase::PreflightComplete,
            ToolExecutionPhase::ResultAvailable,
        );
    }

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
            prepared.id.clone(),
            prepared.name.clone(),
            prepared.args.clone(),
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

    prepared.lifecycle.transition(
        ToolExecutionPhase::ResultAvailable,
        ToolExecutionPhase::ResultHooksApplied,
    );

    let _ = send_extension_event_safe(
        env.extensions,
        Event::ToolExecutionEnd(ToolExecutionEndEvent::new(
            prepared.id.clone(),
            prepared.name.clone(),
            prepared.args.clone(),
            content.clone(),
            details.clone(),
            is_error,
        )),
        env.event_tx,
        "ToolExecutionEnd",
    )
    .await;
    prepared.lifecycle.transition(
        ToolExecutionPhase::ResultHooksApplied,
        ToolExecutionPhase::ExecutionEnded,
    );

    persist_tool_result(
        env,
        &prepared.id,
        &prepared.name,
        content,
        details,
        artifact_path,
        is_error,
    )
    .await?;
    prepared.lifecycle.transition(
        ToolExecutionPhase::ExecutionEnded,
        ToolExecutionPhase::Persisted,
    );

    Ok(())
}

pub(super) async fn append_cancelled_tool_results(
    collected: &CollectedResponse,
    env: ToolExecutionEnv<'_>,
) -> Result<()> {
    for tool_call in &collected.tool_calls {
        persist_tool_result(
            &env,
            &tool_call.id,
            &tool_call.name,
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
    tool_call_id: &str,
    tool_name: &str,
    content: Vec<ContentBlock>,
    details: Option<serde_json::Value>,
    artifact_path: Option<String>,
    is_error: bool,
) -> Result<()> {
    let _ = env.event_tx.send(TurnEvent::ToolResult {
        id: tool_call_id.to_string(),
        name: tool_name.to_string(),
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
            tool_call_id: tool_call_id.to_string(),
            tool_name: tool_name.to_string(),
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

fn scheduler_partial_result(
    scheduling: &ToolScheduling,
    state: &str,
    message: &str,
) -> serde_json::Value {
    let mut details = serde_json::Map::new();
    details.insert("schedulerState".to_string(), serde_json::Value::from(state));
    details.insert(
        "schedulerMessage".to_string(),
        serde_json::Value::from(message.to_string()),
    );
    match scheduling {
        ToolScheduling::ReadOnly => {
            details.insert(
                "scheduling".to_string(),
                serde_json::Value::from("read_only"),
            );
        }
        ToolScheduling::MutatingUnknown => {
            details.insert(
                "scheduling".to_string(),
                serde_json::Value::from("mutating_unknown"),
            );
        }
        ToolScheduling::MutatingPaths(paths) => {
            details.insert(
                "scheduling".to_string(),
                serde_json::Value::from("mutating_paths"),
            );
            details.insert(
                "pathCount".to_string(),
                serde_json::Value::from(paths.len() as u64),
            );
            details.insert(
                "paths".to_string(),
                serde_json::Value::from(
                    paths
                        .iter()
                        .map(|path| path.display().to_string())
                        .collect::<Vec<_>>(),
                ),
            );
        }
    }
    serde_json::json!({ "details": details })
}

async fn send_tool_execution_update(
    env: &ToolExecutionEnv<'_>,
    tool_call_id: &str,
    tool_name: &str,
    input: &serde_json::Value,
    partial_result: serde_json::Value,
) {
    let _ = send_extension_event_safe(
        env.extensions,
        Event::ToolExecutionUpdate(ToolExecutionUpdateEvent::new(
            tool_call_id.to_string(),
            tool_name.to_string(),
            input.clone(),
            partial_result,
        )),
        env.event_tx,
        "ToolExecutionUpdate",
    )
    .await;
}

async fn execute_tool(
    prepared: &mut PreparedToolExecution,
    env: &ToolExecutionEnv<'_>,
    file_queue: &FileQueue,
) -> bb_core::error::BbResult<bb_tools::ToolResult> {
    let normalized_name = normalize_requested_tool_name(&prepared.name);
    let Some(tool) = env
        .tools
        .iter()
        .find(|tool| tool.name() == normalized_name.as_ref())
    else {
        return Err(bb_core::error::BbError::Tool(format!(
            "Unknown tool: {}",
            prepared.name
        )));
    };

    let tool_ctx = tool_context_with_output_forwarding(env, prepared.id.clone());
    let args = prepared.args.clone();
    let scheduling = tool.scheduling(&args, &tool_ctx);

    if !matches!(scheduling, ToolScheduling::ReadOnly) {
        let message = match &scheduling {
            ToolScheduling::MutatingPaths(paths) if paths.len() == 1 => {
                "waiting for file mutation queue"
            }
            ToolScheduling::MutatingPaths(_) => "waiting for file mutation queues",
            ToolScheduling::MutatingUnknown => "waiting for mutation scheduler",
            ToolScheduling::ReadOnly => "running",
        };
        send_tool_execution_update(
            env,
            &prepared.id,
            &prepared.name,
            &prepared.args,
            scheduler_partial_result(&scheduling, "queued", message),
        )
        .await;
    }

    let reservation = file_queue.reserve_scheduling(&scheduling).await;
    send_tool_execution_update(
        env,
        &prepared.id,
        &prepared.name,
        &prepared.args,
        scheduler_partial_result(&scheduling, "running", "running"),
    )
    .await;
    let _ = env.event_tx.send(TurnEvent::ToolExecuting {
        id: prepared.id.clone(),
    });
    prepared.lifecycle.transition(
        ToolExecutionPhase::PreflightComplete,
        ToolExecutionPhase::Running,
    );

    match catch_contained_panics(execute_reserved_tool_call(
        tool.as_ref(),
        args,
        &tool_ctx,
        env.cancel.clone(),
        reservation,
    ))
    .await
    {
        Ok(result) => result,
        Err(message) => Err(bb_core::error::BbError::Tool(format!(
            "Tool {} panicked: {message}",
            prepared.name
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_supports_full_success_path() {
        let mut lifecycle = ToolExecutionStateMachine::new();
        lifecycle.transition(
            ToolExecutionPhase::Created,
            ToolExecutionPhase::ExecutionStarted,
        );
        lifecycle.transition(
            ToolExecutionPhase::ExecutionStarted,
            ToolExecutionPhase::PreflightComplete,
        );
        lifecycle.transition(
            ToolExecutionPhase::PreflightComplete,
            ToolExecutionPhase::Running,
        );
        lifecycle.transition(
            ToolExecutionPhase::Running,
            ToolExecutionPhase::ResultAvailable,
        );
        lifecycle.transition(
            ToolExecutionPhase::ResultAvailable,
            ToolExecutionPhase::ResultHooksApplied,
        );
        lifecycle.transition(
            ToolExecutionPhase::ResultHooksApplied,
            ToolExecutionPhase::ExecutionEnded,
        );
        lifecycle.transition(
            ToolExecutionPhase::ExecutionEnded,
            ToolExecutionPhase::Persisted,
        );

        assert_eq!(lifecycle.phase, ToolExecutionPhase::Persisted);
    }

    #[test]
    fn lifecycle_supports_blocked_preflight_path() {
        let mut lifecycle = ToolExecutionStateMachine::new();
        lifecycle.transition(
            ToolExecutionPhase::Created,
            ToolExecutionPhase::ExecutionStarted,
        );
        lifecycle.transition(
            ToolExecutionPhase::ExecutionStarted,
            ToolExecutionPhase::PreflightComplete,
        );
        lifecycle.transition(
            ToolExecutionPhase::PreflightComplete,
            ToolExecutionPhase::ResultAvailable,
        );
        lifecycle.transition(
            ToolExecutionPhase::ResultAvailable,
            ToolExecutionPhase::ResultHooksApplied,
        );
        lifecycle.transition(
            ToolExecutionPhase::ResultHooksApplied,
            ToolExecutionPhase::ExecutionEnded,
        );
        lifecycle.transition(
            ToolExecutionPhase::ExecutionEnded,
            ToolExecutionPhase::Persisted,
        );

        assert_eq!(lifecycle.phase, ToolExecutionPhase::Persisted);
    }
}
