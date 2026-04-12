//! Tool call preparation, execution, finalization, and sequential/parallel dispatch.

use crate::agent::{
    AfterToolCallContext, AgentAbortSignal, AgentContextSnapshot, AgentEventSink, AgentLoopConfig,
    AgentMessage, AgentMessageContent, AgentMessageRole, AgentTool, BeforeToolCallContext,
    RuntimeAgentEvent,
};
use crate::tool_names::normalize_requested_tool_name;
use anyhow::Result;
use std::sync::Arc;
use tokio::sync::Mutex;

use super::types::{
    AgentToolCall, AgentToolResult, ExecutedToolCallOutcome, ImmediateToolCallOutcome,
    LoopAssistantMessage, PreparedToolCall, ToolResultMessage,
};

pub(crate) async fn execute_tool_calls(
    current_context: &AgentContextSnapshot,
    assistant_message: &LoopAssistantMessage,
    config: &AgentLoopConfig,
    signal: Option<AgentAbortSignal>,
    emit: AgentEventSink,
) -> Result<Vec<ToolResultMessage>> {
    if matches!(
        config.tool_execution,
        crate::agent::ToolExecutionMode::Sequential
    ) {
        execute_tool_calls_sequential(current_context, assistant_message, config, signal, emit)
            .await
    } else {
        execute_tool_calls_parallel(current_context, assistant_message, config, signal, emit).await
    }
}

async fn execute_tool_calls_sequential(
    current_context: &AgentContextSnapshot,
    assistant_message: &LoopAssistantMessage,
    config: &AgentLoopConfig,
    signal: Option<AgentAbortSignal>,
    emit: AgentEventSink,
) -> Result<Vec<ToolResultMessage>> {
    let mut results = Vec::new();

    for tool_call in &assistant_message.tool_calls {
        emit.emit(RuntimeAgentEvent::ToolExecutionStart {
            tool_call_id: tool_call.id.clone(),
        })
        .await?;

        let preparation = prepare_tool_call(
            current_context,
            assistant_message,
            tool_call.clone(),
            config,
            signal.clone(),
        )
        .await;
        match preparation {
            Ok(prepared) => {
                let executed =
                    execute_prepared_tool_call(prepared.clone(), signal.clone(), emit.clone())
                        .await;
                let finalized = finalize_executed_tool_call(
                    current_context,
                    assistant_message,
                    prepared,
                    executed,
                    config,
                    signal.clone(),
                    emit.clone(),
                )
                .await?;
                results.push(finalized);
            }
            Err(immediate) => {
                results.push(
                    emit_tool_call_outcome(
                        tool_call.clone(),
                        immediate.result,
                        immediate.is_error,
                        emit.clone(),
                    )
                    .await?,
                );
            }
        }
    }

    Ok(results)
}

async fn execute_tool_calls_parallel(
    current_context: &AgentContextSnapshot,
    assistant_message: &LoopAssistantMessage,
    config: &AgentLoopConfig,
    signal: Option<AgentAbortSignal>,
    emit: AgentEventSink,
) -> Result<Vec<ToolResultMessage>> {
    let mut immediate_results = Vec::new();
    let mut prepared_calls = Vec::new();

    for tool_call in &assistant_message.tool_calls {
        emit.emit(RuntimeAgentEvent::ToolExecutionStart {
            tool_call_id: tool_call.id.clone(),
        })
        .await?;

        match prepare_tool_call(
            current_context,
            assistant_message,
            tool_call.clone(),
            config,
            signal.clone(),
        )
        .await
        {
            Ok(prepared) => prepared_calls.push(prepared),
            Err(immediate) => {
                immediate_results.push(
                    emit_tool_call_outcome(
                        tool_call.clone(),
                        immediate.result,
                        immediate.is_error,
                        emit.clone(),
                    )
                    .await?,
                );
            }
        }
    }

    let shared_results = Arc::new(Mutex::new(Vec::new()));
    let mut tasks = Vec::new();

    for prepared in prepared_calls {
        let results = Arc::clone(&shared_results);
        let emit = emit.clone();
        let current_context = current_context.clone();
        let assistant_message = assistant_message.clone();
        let config = config.clone();
        let signal = signal.clone();
        tasks.push(tokio::spawn(async move {
            let executed =
                execute_prepared_tool_call(prepared.clone(), signal.clone(), emit.clone()).await;
            let finalized = finalize_executed_tool_call(
                &current_context,
                &assistant_message,
                prepared,
                executed,
                &config,
                signal,
                emit,
            )
            .await?;
            results.lock().await.push(finalized);
            Result::<()>::Ok(())
        }));
    }

    for task in tasks {
        task.await??;
    }

    let mut final_results = immediate_results;
    final_results.extend(shared_results.lock().await.clone());
    Ok(final_results)
}

fn prepare_tool_call_arguments(tool: &AgentTool, tool_call: &AgentToolCall) -> AgentToolCall {
    let _ = tool;
    tool_call.clone()
}

async fn prepare_tool_call(
    current_context: &AgentContextSnapshot,
    assistant_message: &LoopAssistantMessage,
    tool_call: AgentToolCall,
    config: &AgentLoopConfig,
    signal: Option<AgentAbortSignal>,
) -> std::result::Result<PreparedToolCall, ImmediateToolCallOutcome> {
    let normalized_name = normalize_requested_tool_name(&tool_call.name);
    let tool = current_context
        .tools
        .iter()
        .find(|tool| tool.name == normalized_name.as_ref())
        .cloned()
        .ok_or_else(|| ImmediateToolCallOutcome {
            result: create_error_tool_result(format!("Tool {} not found", tool_call.name)),
            is_error: true,
        })?;

    let prepared_tool_call = prepare_tool_call_arguments(&tool, &tool_call);

    if let Some(before_tool_call) = &config.before_tool_call {
        let sig = signal.unwrap_or_else(super::compat::default_abort_signal);
        let _ = before_tool_call(
            BeforeToolCallContext {
                tool_name: Some(prepared_tool_call.name.clone()),
                tool_call_id: Some(prepared_tool_call.id.clone()),
            },
            sig,
        )
        .await;
    }

    let _ = assistant_message;
    Ok(PreparedToolCall {
        tool_call: prepared_tool_call.clone(),
        tool,
        args: prepared_tool_call.arguments,
    })
}

async fn execute_prepared_tool_call(
    prepared: PreparedToolCall,
    _signal: Option<AgentAbortSignal>,
    _emit: AgentEventSink,
) -> ExecutedToolCallOutcome {
    let _ = prepared;

    // Transitional legacy path: bb-core's hidden agent_loop compatibility layer does
    // not execute tools directly, so it returns an explicit tool error result instead.
    ExecutedToolCallOutcome {
        result: create_error_tool_result(
            "bb-core's transitional legacy agent_loop does not execute tools directly",
        ),
        is_error: true,
    }
}

async fn finalize_executed_tool_call(
    current_context: &AgentContextSnapshot,
    assistant_message: &LoopAssistantMessage,
    prepared: PreparedToolCall,
    executed: ExecutedToolCallOutcome,
    config: &AgentLoopConfig,
    signal: Option<AgentAbortSignal>,
    emit: AgentEventSink,
) -> Result<ToolResultMessage> {
    let mut result = executed.result;
    let mut is_error = executed.is_error;

    if let Some(after_tool_call) = &config.after_tool_call {
        let sig = signal.unwrap_or_else(super::compat::default_abort_signal);
        let _ = after_tool_call(
            AfterToolCallContext {
                tool_name: Some(prepared.tool_call.name.clone()),
                tool_call_id: Some(prepared.tool_call.id.clone()),
            },
            sig,
        )
        .await;
    }

    let _ = current_context;
    let _ = assistant_message;
    let _ = prepared.tool;
    let _ = &prepared.args;

    if result.content.is_empty() {
        result = create_error_tool_result("Empty tool result");
        is_error = true;
    }

    emit_tool_call_outcome(prepared.tool_call, result, is_error, emit).await
}

pub(crate) fn create_error_tool_result(message: impl Into<String>) -> AgentToolResult {
    AgentToolResult {
        content: vec![AgentMessageContent::Text(message.into())],
    }
}

async fn emit_tool_call_outcome(
    tool_call: AgentToolCall,
    result: AgentToolResult,
    is_error: bool,
    emit: AgentEventSink,
) -> Result<ToolResultMessage> {
    emit.emit(RuntimeAgentEvent::ToolExecutionEnd {
        tool_call_id: tool_call.id.clone(),
    })
    .await?;

    let tool_result = ToolResultMessage {
        content: result.content.clone(),
        is_error,
        timestamp: chrono::Utc::now().timestamp_millis(),
    };

    emit.emit(RuntimeAgentEvent::MessageStart {
        message: tool_result_to_agent_message(&tool_result),
    })
    .await?;
    emit.emit(RuntimeAgentEvent::MessageEnd {
        message: tool_result_to_agent_message(&tool_result),
    })
    .await?;

    Ok(tool_result)
}

pub(crate) fn tool_result_to_agent_message(result: &ToolResultMessage) -> AgentMessage {
    AgentMessage {
        role: AgentMessageRole::ToolResult,
        content: result.content.clone(),
        api: None,
        provider: None,
        model: None,
        usage: None,
        stop_reason: None,
        error_message: if result.is_error {
            Some(extract_text_content(&result.content))
        } else {
            None
        },
        timestamp: result.timestamp,
    }
}

fn extract_text_content(content: &[AgentMessageContent]) -> String {
    content
        .iter()
        .filter_map(|part| match part {
            AgentMessageContent::Text(text) => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
}
