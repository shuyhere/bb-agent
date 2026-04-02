//! Main agent loop orchestration: `agent_loop`, `agent_loop_continue`,
//! `run_agent_loop`, `run_agent_loop_continue`, and `run_loop`.

use crate::agent::{
    AgentAbortSignal, AgentContextSnapshot, AgentEventSink, AgentFuture, AgentLoopConfig,
    AgentMessage, RuntimeAgentEvent,
};
use anyhow::Result;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};

use super::streaming::stream_assistant_response;
use super::tool_execution::{execute_tool_calls, tool_result_to_agent_message};
use super::types::AgentStream;

/// Start an agent loop with newly-added prompt messages.
pub fn agent_loop(
    prompts: Vec<AgentMessage>,
    context: AgentContextSnapshot,
    config: AgentLoopConfig,
    signal: Option<AgentAbortSignal>,
    stream_fn: Option<crate::agent::StreamFn>,
) -> AgentStream {
    let (event_tx, event_rx) = mpsc::unbounded_channel();
    let (result_tx, result_rx) = oneshot::channel();

    tokio::spawn(async move {
        let sink = AgentEventSink::new(move |event| {
            let event_tx = event_tx.clone();
            Box::pin(async move {
                let _ = event_tx.send(event);
                Ok(())
            })
        });

        let result = run_agent_loop(prompts, context, config, sink, signal, stream_fn).await;
        let _ = result_tx.send(result.unwrap_or_default());
    });

    super::types::AgentEventStream::new(event_rx, result_rx)
}

/// Continue an agent loop without appending a new prompt message first.
pub fn agent_loop_continue(
    context: AgentContextSnapshot,
    config: AgentLoopConfig,
    signal: Option<AgentAbortSignal>,
    stream_fn: Option<crate::agent::StreamFn>,
) -> AgentStream {
    let (event_tx, event_rx) = mpsc::unbounded_channel();
    let (result_tx, result_rx) = oneshot::channel();

    tokio::spawn(async move {
        let sink = AgentEventSink::new(move |event| {
            let event_tx = event_tx.clone();
            Box::pin(async move {
                let _ = event_tx.send(event);
                Ok(())
            })
        });

        let result = run_agent_loop_continue(context, config, sink, signal, stream_fn).await;
        let _ = result_tx.send(result.unwrap_or_default());
    });

    super::types::AgentEventStream::new(event_rx, result_rx)
}

pub async fn run_agent_loop(
    prompts: Vec<AgentMessage>,
    context: AgentContextSnapshot,
    config: AgentLoopConfig,
    emit: AgentEventSink,
    signal: Option<AgentAbortSignal>,
    stream_fn: Option<crate::agent::StreamFn>,
) -> Result<Vec<AgentMessage>> {
    let mut new_messages = prompts.clone();
    let mut current_context = context;
    current_context.messages.extend(prompts.clone());

    emit.emit(RuntimeAgentEvent::MessageStart {
        message: AgentMessage::user_text("[agent_start]"),
    })
    .await?;

    for prompt in prompts {
        emit.emit(RuntimeAgentEvent::MessageStart { message: prompt.clone() })
            .await?;
        emit.emit(RuntimeAgentEvent::MessageEnd { message: prompt })
            .await?;
    }

    run_loop(&mut current_context, &mut new_messages, &config, signal, &emit, stream_fn).await?;

    emit.emit(RuntimeAgentEvent::AgentEnd {
        messages: new_messages.clone(),
    })
    .await?;

    Ok(new_messages)
}

pub async fn run_agent_loop_continue(
    context: AgentContextSnapshot,
    config: AgentLoopConfig,
    emit: AgentEventSink,
    signal: Option<AgentAbortSignal>,
    stream_fn: Option<crate::agent::StreamFn>,
) -> Result<Vec<AgentMessage>> {
    let mut current_context = context;
    let mut new_messages = Vec::new();

    run_loop(&mut current_context, &mut new_messages, &config, signal, &emit, stream_fn).await?;

    emit.emit(RuntimeAgentEvent::AgentEnd {
        messages: new_messages.clone(),
    })
    .await?;

    Ok(new_messages)
}

async fn run_loop(
    current_context: &mut AgentContextSnapshot,
    new_messages: &mut Vec<AgentMessage>,
    config: &AgentLoopConfig,
    signal: Option<AgentAbortSignal>,
    emit: &AgentEventSink,
    stream_fn: Option<crate::agent::StreamFn>,
) -> Result<()> {
    let mut first_turn = true;
    let mut pending_messages = get_pending_messages(&config.get_steering_messages).await;

    loop {
        let mut has_more_tool_calls = true;

        while has_more_tool_calls || !pending_messages.is_empty() {
            if !first_turn {
                emit.emit(RuntimeAgentEvent::TurnEnd {
                    message: AgentMessage::user_text("[turn_start]"),
                })
                .await?;
            } else {
                first_turn = false;
            }

            if !pending_messages.is_empty() {
                for message in pending_messages.drain(..) {
                    emit.emit(RuntimeAgentEvent::MessageStart {
                        message: message.clone(),
                    })
                    .await?;
                    emit.emit(RuntimeAgentEvent::MessageEnd {
                        message: message.clone(),
                    })
                    .await?;
                    current_context.messages.push(message.clone());
                    new_messages.push(message);
                }
            }

            let assistant = stream_assistant_response(current_context, config, signal.clone(), emit.clone(), stream_fn.clone()).await?;
            current_context.messages.push(assistant.message.clone());
            new_messages.push(assistant.message.clone());

            if matches!(assistant.stop_reason.as_deref(), Some("error") | Some("aborted")) {
                emit.emit(RuntimeAgentEvent::TurnEnd {
                    message: assistant.message,
                })
                .await?;
                return Ok(());
            }

            has_more_tool_calls = !assistant.tool_calls.is_empty();
            let mut tool_results = Vec::new();

            if has_more_tool_calls {
                tool_results = execute_tool_calls(
                    current_context,
                    &assistant,
                    config,
                    signal.clone(),
                    emit.clone(),
                )
                .await?;

                for result in &tool_results {
                    let message = tool_result_to_agent_message(result);
                    current_context.messages.push(message.clone());
                    new_messages.push(message);
                }
            }

            emit.emit(RuntimeAgentEvent::TurnEnd {
                message: assistant.message,
            })
            .await?;

            pending_messages = get_pending_messages(&config.get_steering_messages).await;
        }

        let follow_up_messages = get_pending_messages(&config.get_follow_up_messages).await;
        if !follow_up_messages.is_empty() {
            pending_messages = follow_up_messages;
            continue;
        }

        break;
    }

    Ok(())
}

async fn get_pending_messages(
    getter: &Option<Arc<dyn Fn() -> AgentFuture<Vec<AgentMessage>> + Send + Sync>>,
) -> Vec<AgentMessage> {
    match getter {
        Some(getter) => getter().await,
        None => Vec::new(),
    }
}
