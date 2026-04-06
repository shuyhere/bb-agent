//! Assistant response streaming helpers.

use crate::agent::{
    AgentAbortSignal, AgentContextSnapshot, AgentEventSink, AgentLoopConfig, AgentMessage,
    AgentMessageContent, AgentMessageRole, RuntimeAgentEvent,
};
use anyhow::Result;

use super::types::LoopAssistantMessage;

pub(crate) async fn stream_assistant_response(
    context: &mut AgentContextSnapshot,
    config: &AgentLoopConfig,
    signal: Option<AgentAbortSignal>,
    emit: AgentEventSink,
    stream_fn: Option<crate::agent::StreamFn>,
) -> Result<LoopAssistantMessage> {
    let mut messages = context.messages.clone();
    if let Some(transform) = &config.transform_context {
        let sig = signal
            .clone()
            .unwrap_or_else(super::compat::default_abort_signal);
        messages = transform(messages, sig).await;
    }

    if let Some(convert) = &config.convert_to_llm {
        let _ = convert(messages.clone()).await;
    }

    if let Some(stream_fn) = stream_fn {
        let sink = emit.clone();
        let sig = signal
            .clone()
            .unwrap_or_else(super::compat::default_abort_signal);
        let mut loop_config = config.clone();
        loop_config.convert_to_llm = config.convert_to_llm.clone();
        loop_config.transform_context = config.transform_context.clone();
        stream_fn(context.clone(), loop_config, sink, sig).await?;
    }

    // Transitional legacy path: until the stream/runtime layers are unified,
    // synthesize a minimal assistant completion record after the stream callback runs.
    let message = AgentMessage {
        role: AgentMessageRole::Assistant,
        content: vec![AgentMessageContent::Text(String::new())],
        api: Some(config.model.api.clone()),
        provider: Some(config.model.provider.clone()),
        model: Some(config.model.id.clone()),
        usage: None,
        stop_reason: Some("completed".to_string()),
        error_message: None,
        timestamp: chrono::Utc::now().timestamp_millis(),
    };

    emit.emit(RuntimeAgentEvent::MessageStart {
        message: message.clone(),
    })
    .await?;
    emit.emit(RuntimeAgentEvent::MessageEnd {
        message: message.clone(),
    })
    .await?;

    Ok(LoopAssistantMessage {
        message,
        tool_calls: Vec::new(),
        stop_reason: Some("completed".to_string()),
    })
}
