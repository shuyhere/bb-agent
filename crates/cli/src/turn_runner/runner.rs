use anyhow::Result;
use bb_core::agent_loop::is_context_overflow;
use bb_core::agent_session::messages_to_provider;
use bb_core::types::AgentMessage;
use bb_hooks::Event;
use bb_provider::{
    CollectedResponse, CompletionRequest, ProviderRetryEvent, RequestOptions, RetryCallback,
    StreamEvent,
};
use bb_session::context;
use tokio::sync::mpsc;

use super::TurnConfig;
use super::TurnEvent;
use super::hooks::send_extension_event_safe;
use super::panic::catch_contained_panics;
use super::persistence::{append_assistant_message, append_custom_message};
use super::tools::{ToolExecutionEnv, execute_tool_calls};

struct StreamCollection {
    events: Vec<StreamEvent>,
    context_overflow_error: Option<String>,
}

pub(crate) async fn run_turn(
    config: TurnConfig,
    event_tx: mpsc::UnboundedSender<TurnEvent>,
    user_prompt: String,
) -> (TurnConfig, Result<()>) {
    let result = catch_contained_panics(run_turn_inner(&config, &event_tx, &user_prompt)).await;

    let result = match result {
        Ok(result) => result,
        Err(message) => {
            let message = format!("turn runner panicked: {message}");
            let _ = event_tx.send(TurnEvent::Error(message.clone()));
            let _ = catch_contained_panics(config.extensions.send_event(&Event::AgentEnd)).await;
            Err(anyhow::anyhow!(message))
        }
    };

    (config, result)
}

pub(crate) async fn run_turn_inner(
    config: &TurnConfig,
    event_tx: &mpsc::UnboundedSender<TurnEvent>,
    user_prompt: &str,
) -> Result<()> {
    let mut turn_index: u32 = 0;
    let mut system_prompt = config.system_prompt.clone();

    if let Some(result) = send_extension_event_safe(
        &config.extensions,
        Event::BeforeAgentStart {
            prompt: user_prompt.to_string(),
            system_prompt: system_prompt.clone(),
        },
        event_tx,
        "BeforeAgentStart",
    )
    .await
    {
        if let Some(updated_prompt) = result.system_prompt {
            system_prompt = updated_prompt;
        }
        if let Some(message) = result.message {
            append_custom_message(&config.conn, &config.session_id, message).await?;
        }
    }

    loop {
        let _ = event_tx.send(TurnEvent::TurnStart { turn_index });
        let _ = send_extension_event_safe(
            &config.extensions,
            Event::TurnStart { turn_index },
            event_tx,
            "TurnStart",
        )
        .await;

        if config.cancel.is_cancelled() {
            let _ = event_tx.send(TurnEvent::Done {
                text: String::new(),
            });
            break;
        }

        let request = build_request(config, event_tx, &system_prompt).await?;
        let stream = collect_stream_events(config, event_tx, request).await?;

        if config.cancel.is_cancelled() {
            let _ = event_tx.send(TurnEvent::Done {
                text: String::new(),
            });
            break;
        }

        if let Some(message) = stream.context_overflow_error {
            let _ = event_tx.send(TurnEvent::ContextOverflow { message });
            break;
        }

        let collected = CollectedResponse::from_events(&stream.events);
        {
            let conn = config.conn.lock().await;
            append_assistant_message(&conn, &config.session_id, &config.model, &collected)?;
        }

        let _ = event_tx.send(TurnEvent::TurnEnd);
        let _ = send_extension_event_safe(
            &config.extensions,
            Event::TurnEnd { turn_index },
            event_tx,
            "TurnEnd",
        )
        .await;

        if collected.tool_calls.is_empty() || config.cancel.is_cancelled() {
            let _ = event_tx.send(TurnEvent::Done {
                text: collected.text,
            });
            break;
        }

        execute_tool_calls(
            &collected,
            ToolExecutionEnv {
                conn: &config.conn,
                session_id: &config.session_id,
                tools: &config.tools,
                tool_ctx: &config.tool_ctx,
                cancel: &config.cancel,
                extensions: &config.extensions,
                event_tx,
            },
        )
        .await?;

        turn_index += 1;
    }

    let _ =
        send_extension_event_safe(&config.extensions, Event::AgentEnd, event_tx, "AgentEnd").await;
    Ok(())
}

async fn build_request(
    config: &TurnConfig,
    event_tx: &mpsc::UnboundedSender<TurnEvent>,
    system_prompt: &str,
) -> Result<CompletionRequest> {
    let conn = config.conn.lock().await;
    let context = context::build_context(&conn, &config.session_id)?;
    drop(conn);

    let provider_messages =
        messages_to_provider(&apply_context_hook(config, event_tx, context.messages).await?);

    let mut request = CompletionRequest {
        system_prompt: system_prompt.to_string(),
        messages: provider_messages,
        tools: config.tool_defs.clone(),
        extra_tool_schemas: vec![],
        model: config.model.id.clone(),
        max_tokens: Some(config.model.max_tokens as u32),
        stream: true,
        api: config.model.api.clone(),
        thinking: config.thinking.clone(),
    };

    if let Some(result) = send_extension_event_safe(
        &config.extensions,
        Event::BeforeProviderRequest {
            payload: serde_json::to_value(&request).unwrap_or_default(),
        },
        event_tx,
        "BeforeProviderRequest",
    )
    .await
        && let Some(payload) = result.payload
        && let Ok(updated_request) = serde_json::from_value::<CompletionRequest>(payload)
    {
        request = updated_request;
    }

    Ok(request)
}

async fn apply_context_hook(
    config: &TurnConfig,
    event_tx: &mpsc::UnboundedSender<TurnEvent>,
    mut messages: Vec<AgentMessage>,
) -> Result<Vec<AgentMessage>> {
    if let Some(result) = send_extension_event_safe(
        &config.extensions,
        Event::Context(bb_hooks::events::ContextEvent {
            messages: messages.clone(),
        }),
        event_tx,
        "Context",
    )
    .await
        && let Some(replacement) = result.messages
    {
        messages = replacement
            .into_iter()
            .filter_map(|message| serde_json::from_value::<AgentMessage>(message).ok())
            .collect();
    }

    Ok(messages)
}

async fn collect_stream_events(
    config: &TurnConfig,
    event_tx: &mpsc::UnboundedSender<TurnEvent>,
    request: CompletionRequest,
) -> Result<StreamCollection> {
    let (stream_tx, mut stream_rx) = mpsc::unbounded_channel();
    let provider = config.provider.clone();
    let stream_cancel = config.cancel.clone();
    let options = build_request_options(config, event_tx.clone());

    let stream_handle = tokio::spawn(async move {
        let result = catch_contained_panics(provider.stream(request, options, stream_tx)).await;
        match result {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => {
                if !stream_cancel.is_cancelled() {
                    Err(error)
                } else {
                    Ok(())
                }
            }
            Err(message) => {
                if !stream_cancel.is_cancelled() {
                    Err(bb_core::error::BbError::Provider(format!(
                        "provider stream panicked: {message}"
                    )))
                } else {
                    Ok(())
                }
            }
        }
    });

    let mut events = Vec::new();
    let mut context_overflow_error = None;

    while let Some(event) = stream_rx.recv().await {
        forward_stream_event(event_tx, &event, &mut context_overflow_error);
        events.push(event);

        if config.cancel.is_cancelled() {
            break;
        }
    }

    match stream_handle.await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            if !config.cancel.is_cancelled() {
                let message = error.to_string();
                let _ = event_tx.send(TurnEvent::Error(message.clone()));
                return Err(anyhow::anyhow!(message));
            }
        }
        Err(error) => {
            if !config.cancel.is_cancelled() {
                let message = format!("stream task failed: {error}");
                let _ = event_tx.send(TurnEvent::Error(message.clone()));
                return Err(anyhow::anyhow!(message));
            }
        }
    }

    Ok(StreamCollection {
        events,
        context_overflow_error,
    })
}

fn build_request_options(
    config: &TurnConfig,
    event_tx: mpsc::UnboundedSender<TurnEvent>,
) -> RequestOptions {
    let retry_callback: RetryCallback = std::sync::Arc::new(move |event| {
        let turn_event = match event {
            ProviderRetryEvent::Start {
                attempt,
                max_attempts,
                delay_ms,
                error_message,
            } => TurnEvent::AutoRetryStart {
                attempt,
                max_attempts,
                delay_ms,
                error_message,
            },
            ProviderRetryEvent::End { .. } => TurnEvent::AutoRetryEnd,
        };
        let _ = event_tx.send(turn_event);
    });

    RequestOptions {
        api_key: config.api_key.clone(),
        base_url: config.base_url.clone(),
        headers: config.headers.clone(),
        cancel: config.cancel.clone(),
        retry_callback: Some(retry_callback),
        max_retries: if config.retry_enabled {
            config.retry_max_retries.max(1)
        } else {
            1
        },
        retry_base_delay_ms: config.retry_base_delay_ms,
        max_retry_delay_ms: config.retry_max_delay_ms,
    }
}

fn forward_stream_event(
    event_tx: &mpsc::UnboundedSender<TurnEvent>,
    event: &StreamEvent,
    context_overflow_error: &mut Option<String>,
) {
    match event {
        StreamEvent::TextDelta { text } => {
            let _ = event_tx.send(TurnEvent::TextDelta(text.clone()));
        }
        StreamEvent::ThinkingDelta { text } => {
            let _ = event_tx.send(TurnEvent::ThinkingDelta(text.clone()));
        }
        StreamEvent::ToolCallStart { id, name } => {
            let _ = event_tx.send(TurnEvent::ToolCallStart {
                id: id.clone(),
                name: name.clone(),
            });
        }
        StreamEvent::ToolCallDelta {
            id,
            arguments_delta,
        } => {
            let _ = event_tx.send(TurnEvent::ToolCallDelta {
                id: id.clone(),
                args: arguments_delta.clone(),
            });
        }
        StreamEvent::Error { message } => {
            if is_context_overflow(message) {
                *context_overflow_error = Some(message.clone());
            }
            let _ = event_tx.send(TurnEvent::Error(message.clone()));
        }
        _ => {}
    }
}
