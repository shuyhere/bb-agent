//! Agent loop structure ported from pi's `packages/agent/src/agent-loop.ts`.
//!
//! This module now contains a Rust equivalent of pi's top-level loop boundaries:
//! - `agent_loop`
//! - `agent_loop_continue`
//! - `run_agent_loop`
//! - `run_agent_loop_continue`
//! - helper functions for loop execution, assistant streaming, tool preparation,
//!   tool execution, finalization, and result emission
//!
//! The concrete LLM/tool runtime in BB-Agent is still split across older layers,
//! so several parts remain TODO-safe placeholders. The architecture and function
//! boundaries are intentionally kept close to pi so later wiring can be done
//! without redesigning the module shape again.

use crate::agent::{
    AfterToolCallContext, AgentAbortSignal, AgentContextSnapshot, AgentEventSink, AgentFuture,
    AgentLoopConfig, AgentMessage, AgentMessageContent, AgentMessageRole, AgentTool, BeforeToolCallContext,
    RuntimeAgentEvent,
};
use anyhow::Result;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, Mutex};

/// Legacy UI-facing event type still used by existing BB-Agent layers.
#[derive(Clone, Debug)]
pub enum AgentLoopEvent {
    TurnStart { turn_index: u32 },
    TextDelta { text: String },
    ThinkingDelta { text: String },
    ToolCallStart { id: String, name: String },
    ToolCallDelta { id: String, args_delta: String },
    ToolExecuting { id: String, name: String },
    ToolResult { id: String, name: String, content: String, is_error: bool },
    TurnEnd { turn_index: u32 },
    AssistantDone,
    Error { message: String },
}

/// Legacy context usage information still used by existing BB-Agent layers.
#[derive(Clone, Debug)]
pub struct ContextUsage {
    pub tokens: u64,
    pub context_window: u64,
    pub percent: f64,
}

/// Pi-style event stream replacement for Rust.
pub struct AgentEventStream<TEvent, TResult> {
    receiver: mpsc::UnboundedReceiver<TEvent>,
    result: oneshot::Receiver<TResult>,
}

impl<TEvent, TResult> AgentEventStream<TEvent, TResult> {
    pub fn new(
        receiver: mpsc::UnboundedReceiver<TEvent>,
        result: oneshot::Receiver<TResult>,
    ) -> Self {
        Self { receiver, result }
    }

    pub async fn recv(&mut self) -> Option<TEvent> {
        self.receiver.recv().await
    }

    pub async fn result(self) -> std::result::Result<TResult, oneshot::error::RecvError> {
        self.result.await
    }
}

pub type AgentStream = AgentEventStream<RuntimeAgentEvent, Vec<AgentMessage>>;

#[derive(Clone, Debug, Default)]
pub struct AgentToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

#[derive(Clone, Debug, Default)]
pub struct AgentToolResult {
    pub content: Vec<AgentMessageContent>,
    pub details: Value,
}

#[derive(Clone, Debug, Default)]
pub struct ToolResultMessage {
    pub tool_call_id: String,
    pub tool_name: String,
    pub content: Vec<AgentMessageContent>,
    pub details: Value,
    pub is_error: bool,
    pub timestamp: i64,
}

#[derive(Clone, Debug)]
pub struct LoopAssistantMessage {
    pub message: AgentMessage,
    pub tool_calls: Vec<AgentToolCall>,
    pub stop_reason: Option<String>,
}

pub type LoopEventSink = Arc<dyn Fn(RuntimeAgentEvent) -> AgentFuture<Result<()>> + Send + Sync>;

#[derive(Clone, Debug)]
struct PreparedToolCall {
    tool_call: AgentToolCall,
    tool: AgentTool,
    args: Value,
}

#[derive(Clone, Debug)]
struct ImmediateToolCallOutcome {
    result: AgentToolResult,
    is_error: bool,
}

#[derive(Clone, Debug)]
struct ExecutedToolCallOutcome {
    result: AgentToolResult,
    is_error: bool,
}

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

    AgentEventStream::new(event_rx, result_rx)
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

    AgentEventStream::new(event_rx, result_rx)
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

async fn stream_assistant_response(
    context: &mut AgentContextSnapshot,
    config: &AgentLoopConfig,
    signal: Option<AgentAbortSignal>,
    emit: AgentEventSink,
    stream_fn: Option<crate::agent::StreamFn>,
) -> Result<LoopAssistantMessage> {
    let mut messages = context.messages.clone();
    if let Some(transform) = &config.transform_context {
        let sig = signal.clone().unwrap_or_else(default_abort_signal);
        messages = transform(messages, sig).await;
    }

    if let Some(convert) = &config.convert_to_llm {
        let _ = convert(messages.clone()).await;
    }

    if let Some(stream_fn) = stream_fn {
        let sink = emit.clone();
        let sig = signal.clone().unwrap_or_else(default_abort_signal);
        let mut loop_config = config.clone();
        loop_config.convert_to_llm = config.convert_to_llm.clone();
        loop_config.transform_context = config.transform_context.clone();
        stream_fn(context.clone(), loop_config, sink, sig).await?;
    }

    // TODO: Replace placeholder assistant synthesis with true provider-backed
    // assistant message construction once the stream/runtime layers are unified.
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

async fn execute_tool_calls(
    current_context: &AgentContextSnapshot,
    assistant_message: &LoopAssistantMessage,
    config: &AgentLoopConfig,
    signal: Option<AgentAbortSignal>,
    emit: AgentEventSink,
) -> Result<Vec<ToolResultMessage>> {
    if matches!(config.tool_execution, crate::agent::ToolExecutionMode::Sequential) {
        execute_tool_calls_sequential(current_context, assistant_message, config, signal, emit).await
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

        let preparation = prepare_tool_call(current_context, assistant_message, tool_call.clone(), config, signal.clone()).await;
        match preparation {
            Ok(prepared) => {
                let executed = execute_prepared_tool_call(prepared.clone(), signal.clone(), emit.clone()).await;
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
                results.push(emit_tool_call_outcome(tool_call.clone(), immediate.result, immediate.is_error, emit.clone()).await?);
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

        match prepare_tool_call(current_context, assistant_message, tool_call.clone(), config, signal.clone()).await {
            Ok(prepared) => prepared_calls.push(prepared),
            Err(immediate) => {
                immediate_results.push(emit_tool_call_outcome(tool_call.clone(), immediate.result, immediate.is_error, emit.clone()).await?);
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
            let executed = execute_prepared_tool_call(prepared.clone(), signal.clone(), emit.clone()).await;
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
    let tool = current_context
        .tools
        .iter()
        .find(|tool| tool.name == tool_call.name)
        .cloned()
        .ok_or_else(|| ImmediateToolCallOutcome {
            result: create_error_tool_result(format!("Tool {} not found", tool_call.name)),
            is_error: true,
        })?;

    let prepared_tool_call = prepare_tool_call_arguments(&tool, &tool_call);

    if let Some(before_tool_call) = &config.before_tool_call {
        let sig = signal.unwrap_or_else(default_abort_signal);
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

    // TODO: Wire real tool execution once AgentTool carries execution handlers in bb-core.
    ExecutedToolCallOutcome {
        result: create_error_tool_result("Tool execution not yet wired in bb-core agent_loop"),
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
        let sig = signal.unwrap_or_else(default_abort_signal);
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

fn create_error_tool_result(message: impl Into<String>) -> AgentToolResult {
    AgentToolResult {
        content: vec![AgentMessageContent::Text(message.into())],
        details: Value::Object(Default::default()),
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
        tool_call_id: tool_call.id.clone(),
        tool_name: tool_call.name.clone(),
        content: result.content.clone(),
        details: result.details.clone(),
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

fn tool_result_to_agent_message(result: &ToolResultMessage) -> AgentMessage {
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

async fn get_pending_messages(
    getter: &Option<Arc<dyn Fn() -> AgentFuture<Vec<AgentMessage>> + Send + Sync>>,
) -> Vec<AgentMessage> {
    match getter {
        Some(getter) => getter().await,
        None => Vec::new(),
    }
}

fn default_abort_signal() -> AgentAbortSignal {
    crate::agent::AgentAbortController::new().signal()
}

// ── Legacy CLI/session compatibility helpers ───────────────────────────────

/// Check if a provider error message indicates a context overflow.
///
/// This legacy string matcher is still used by the CLI/session compatibility
/// shim while the runtime loop migration is in progress.
pub fn is_context_overflow(msg: &str) -> bool {
    let msg_lower = msg.to_lowercase();
    msg_lower.contains("context_length_exceeded")
        || msg_lower.contains("maximum context length")
        || msg_lower.contains("too many tokens")
        || msg_lower.contains("request too large")
        || msg_lower.contains("prompt is too long")
        || (msg_lower.contains("400") && msg_lower.contains("token"))
}

/// Check if a provider error message indicates rate limiting.
pub fn is_rate_limited(msg: &str) -> bool {
    msg.contains("429") || msg.to_lowercase().contains("rate limit")
}

/// Convert legacy session messages into provider request JSON.
///
/// The canonical implementation lives in `bb_core::agent_loop` even though the
/// underlying conversion logic is shared with `agent_session`.
pub fn messages_to_provider(messages: &[crate::types::AgentMessage]) -> Vec<serde_json::Value> {
    crate::agent_session::messages_to_provider(messages)
}

/// Minimal legacy steering/follow-up queue retained for CLI compatibility.
#[derive(Debug, Default)]
pub struct MessageQueue {
    steers: Vec<String>,
    follow_ups: Vec<String>,
}

impl MessageQueue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push_steer(&mut self, text: String) {
        self.steers.push(text);
    }

    pub fn push_follow_up(&mut self, text: String) {
        self.follow_ups.push(text);
    }

    pub fn take_steers(&mut self) -> Vec<String> {
        std::mem::take(&mut self.steers)
    }

    pub fn take_follow_ups(&mut self) -> Vec<String> {
        std::mem::take(&mut self.follow_ups)
    }

    pub fn is_empty(&self) -> bool {
        self.steers.is_empty() && self.follow_ups.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::{is_context_overflow, is_rate_limited, MessageQueue};

    #[test]
    fn test_is_context_overflow() {
        assert!(is_context_overflow("HTTP 400: context_length_exceeded"));
        assert!(is_context_overflow("maximum context length is 200000 tokens"));
        assert!(is_context_overflow("too many tokens in the request"));
        assert!(is_context_overflow("request too large for model"));
        assert!(is_context_overflow("prompt is too long"));
        assert!(is_context_overflow("HTTP 400: token limit exceeded"));
        assert!(!is_context_overflow("HTTP 401: Unauthorized"));
        assert!(!is_context_overflow("HTTP 500: Internal Server Error"));
    }

    #[test]
    fn test_is_rate_limited() {
        assert!(is_rate_limited("HTTP 429: Rate limit exceeded"));
        assert!(is_rate_limited("rate limit reached"));
        assert!(is_rate_limited("429 Too Many Requests"));
        assert!(!is_rate_limited("HTTP 400: Bad request"));
        assert!(!is_rate_limited("HTTP 500: Internal Server Error"));
    }

    #[test]
    fn test_message_queue() {
        let mut q = MessageQueue::new();
        assert!(q.is_empty());

        q.push_steer("fix the bug".into());
        q.push_follow_up("then run tests".into());
        q.push_steer("also check imports".into());

        assert!(!q.is_empty());

        let steers = q.take_steers();
        assert_eq!(steers.len(), 2);
        assert_eq!(steers[0], "fix the bug");
        assert_eq!(steers[1], "also check imports");

        let follow_ups = q.take_follow_ups();
        assert_eq!(follow_ups.len(), 1);
        assert_eq!(follow_ups[0], "then run tests");

        assert!(q.is_empty());
    }

    #[test]
    fn test_message_queue_empty_operations() {
        let mut q = MessageQueue::new();
        assert!(q.take_steers().is_empty());
        assert!(q.take_follow_ups().is_empty());
        assert!(q.is_empty());
    }
}
