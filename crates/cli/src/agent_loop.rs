//! The inner turn loop of the agent.
//!
//! Takes a session with messages already appended, streams LLM responses,
//! executes tool calls, and loops until the assistant is done.

use std::time::Duration;

use bb_core::agent;
use bb_core::agent_loop::AgentLoopEvent;
use bb_core::types::*;
use bb_hooks::{Event, ToolCallEvent, ToolResultEvent, ContextEvent, SharedEventBus};
use bb_provider::streaming::CollectedResponse;
use bb_provider::{CompletionRequest, Provider, RequestOptions, StreamEvent};
use bb_session::{compaction, context, store, tree};
use bb_tools::{Tool, ToolContext};
use chrono::Utc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// Run the agent turn loop.
///
/// Assumes the user message has already been appended to the session.
/// Streams events to `event_tx` and loops until the assistant produces
/// a response with no tool calls.
pub async fn run_agent_loop(
    conn: &rusqlite::Connection,
    session_id: &str,
    system_prompt: &str,
    model: &bb_provider::registry::Model,
    provider: &dyn Provider,
    api_key: &str,
    base_url: &str,
    tools: &[Box<dyn Tool>],
    tool_defs: &[serde_json::Value],
    tool_ctx: &ToolContext,
    event_tx: &mpsc::UnboundedSender<AgentLoopEvent>,
    event_bus: &SharedEventBus,
) -> anyhow::Result<()> {
    let mut turn_index: u32 = 0;
    let mut active_system_prompt = system_prompt.to_string();

    loop {
        // Step 1: Send TurnStart
        let _ = event_tx.send(AgentLoopEvent::TurnStart { turn_index });

        // Fire turn_start hook
        let _ = event_bus.emit(&Event::TurnStart { turn_index }).await;

        // Step 2: Build context from session
        let ctx = context::build_context(conn, session_id)?;

        // Fire context hook — extensions can replace the message list
        let mut final_messages = ctx.messages.clone();
        let context_event = Event::Context(ContextEvent { messages: ctx.messages.clone() });
        if let Some(result) = event_bus.emit(&context_event).await {
            if let Some(new_msgs_json) = result.messages {
                // Try to deserialize replacement messages
                if let Ok(parsed) = serde_json::from_value::<Vec<AgentMessage>>(
                    serde_json::Value::Array(new_msgs_json),
                ) {
                    final_messages = parsed;
                }
            }
        }

        let provider_messages = messages_to_provider(&final_messages);

        // Step 3: Build completion request
        let request = CompletionRequest {
            system_prompt: active_system_prompt.clone(),
            messages: provider_messages,
            tools: tool_defs.to_vec(),
            model: model.id.clone(),
            max_tokens: Some(model.max_tokens as u32),
            stream: true,
            thinking: None,
        };

        let options = RequestOptions {
            api_key: api_key.to_string(),
            base_url: base_url.to_string(),
            headers: std::collections::HashMap::new(),
            cancel: CancellationToken::new(),
        };

        // Step 4: Stream from provider, forwarding events
        let (stream_tx, mut stream_rx) = mpsc::unbounded_channel();

        let stream_result = provider.stream(request, options, stream_tx).await;
        if let Err(e) = stream_result {
            let msg = format!("{e}");

            // Context overflow: auto-compact and retry
            if is_context_overflow(&msg) {
                tracing::warn!("Context overflow detected, auto-compacting...");
                let _ = event_tx.send(AgentLoopEvent::Error {
                    message: "Context overflow detected, auto-compacting...".into(),
                });
                let compaction_settings = CompactionSettings::default();
                let path = tree::active_path(conn, session_id)?;
                if let Some(prep) = compaction::prepare_compaction(&path, &compaction_settings) {
                    let cancel_compact = CancellationToken::new();
                    let result = compaction::compact(
                        &prep, provider, &model.id, api_key, base_url,
                        None, cancel_compact,
                    ).await?;
                    let comp_entry = SessionEntry::Compaction {
                        base: EntryBase {
                            id: EntryId::generate(),
                            parent_id: get_leaf(conn, session_id),
                            timestamp: Utc::now(),
                        },
                        summary: result.summary,
                        first_kept_entry_id: EntryId(result.first_kept_entry_id),
                        tokens_before: result.tokens_before,
                        details: Some(serde_json::json!({
                            "readFiles": result.read_files,
                            "modifiedFiles": result.modified_files,
                        })),
                        from_plugin: false,
                    };
                    store::append_entry(conn, session_id, &comp_entry)?;
                    let _ = event_tx.send(AgentLoopEvent::Error {
                        message: format!("[c] Auto-compacted ({} tokens summarized), retrying...", result.tokens_before),
                    });
                    continue; // retry the turn
                } else {
                    let _ = event_tx.send(AgentLoopEvent::Error {
                        message: "Context overflow but nothing to compact".into(),
                    });
                    return Err(anyhow::anyhow!("Context overflow but nothing to compact"));
                }
            }

            // Rate limit: wait and retry
            if is_rate_limited(&msg) {
                tracing::warn!("Rate limited, waiting 10 seconds...");
                let _ = event_tx.send(AgentLoopEvent::Error {
                    message: "Rate limited, waiting 10 seconds...".into(),
                });
                tokio::time::sleep(Duration::from_secs(10)).await;
                continue; // retry the turn
            }

            let _ = event_tx.send(AgentLoopEvent::Error {
                message: format!("Provider error: {e}"),
            });
            return Err(e.into());
        }

        // Collect events while forwarding to event_tx
        let mut all_events = Vec::new();
        while let Some(event) = stream_rx.recv().await {
            match &event {
                StreamEvent::TextDelta { text } => {
                    let _ = event_tx.send(AgentLoopEvent::TextDelta { text: text.clone() });
                }
                StreamEvent::ThinkingDelta { text } => {
                    let _ = event_tx.send(AgentLoopEvent::ThinkingDelta { text: text.clone() });
                }
                StreamEvent::ToolCallStart { id, name } => {
                    let _ = event_tx.send(AgentLoopEvent::ToolCallStart {
                        id: id.clone(),
                        name: name.clone(),
                    });
                }
                StreamEvent::ToolCallDelta { id, arguments_delta } => {
                    let _ = event_tx.send(AgentLoopEvent::ToolCallDelta {
                        id: id.clone(),
                        args_delta: arguments_delta.clone(),
                    });
                }
                StreamEvent::Error { message } => {
                    let _ = event_tx.send(AgentLoopEvent::Error {
                        message: message.clone(),
                    });
                }
                _ => {}
            }
            all_events.push(event);
        }

        // Step 5: Collect final response, build AssistantMessage, append to session
        let collected = CollectedResponse::from_events(&all_events);

        let mut assistant_content = Vec::new();
        if !collected.thinking.is_empty() {
            assistant_content.push(AssistantContent::Thinking {
                thinking: collected.thinking,
            });
        }
        if !collected.text.is_empty() {
            assistant_content.push(AssistantContent::Text {
                text: collected.text,
            });
        }
        for tc in &collected.tool_calls {
            let args: serde_json::Value =
                serde_json::from_str(&tc.arguments).unwrap_or(serde_json::json!({}));
            assistant_content.push(AssistantContent::ToolCall {
                id: tc.id.clone(),
                name: tc.name.clone(),
                arguments: args,
            });
        }

        let assistant_msg = AgentMessage::Assistant(AssistantMessage {
            content: assistant_content,
            provider: model.provider.clone(),
            model: model.id.clone(),
            usage: Usage {
                input: collected.input_tokens,
                output: collected.output_tokens,
                ..Default::default()
            },
            stop_reason: if collected.tool_calls.is_empty() {
                StopReason::Stop
            } else {
                StopReason::ToolUse
            },
            error_message: None,
            timestamp: Utc::now().timestamp_millis(),
        });

        let asst_entry = SessionEntry::Message {
            base: EntryBase {
                id: EntryId::generate(),
                parent_id: get_leaf(conn, session_id),
                timestamp: Utc::now(),
            },
            message: assistant_msg,
        };
        store::append_entry(conn, session_id, &asst_entry)?;

        // Step 7: If no tool calls, send AssistantDone and exit loop
        if collected.tool_calls.is_empty() {
            let _ = event_tx.send(AgentLoopEvent::TurnEnd { turn_index });
            let _ = event_tx.send(AgentLoopEvent::AssistantDone);
            break;
        }

        // Step 6: Execute tool calls
        let cancel = CancellationToken::new();
        for tc in &collected.tool_calls {
            let mut args: serde_json::Value =
                serde_json::from_str(&tc.arguments).unwrap_or(serde_json::json!({}));

            // Fire tool_call hook — can block or mutate input
            let tc_event = Event::ToolCall(ToolCallEvent {
                tool_call_id: tc.id.clone(),
                tool_name: tc.name.clone(),
                input: args.clone(),
            });
            if let Some(hook_result) = event_bus.emit(&tc_event).await {
                if hook_result.block == Some(true) {
                    let reason = hook_result.reason.unwrap_or_else(|| "Blocked by extension".into());
                    // Append blocked tool result
                    let blocked_content = vec![ContentBlock::Text { text: reason.clone() }];
                    let _ = event_tx.send(AgentLoopEvent::ToolResult {
                        id: tc.id.clone(),
                        name: tc.name.clone(),
                        content: reason,
                        is_error: true,
                    });
                    let tool_result_msg = AgentMessage::ToolResult(ToolResultMessage {
                        tool_call_id: tc.id.clone(),
                        tool_name: tc.name.clone(),
                        content: blocked_content,
                        details: None,
                        is_error: true,
                        timestamp: Utc::now().timestamp_millis(),
                    });
                    let tr_entry = SessionEntry::Message {
                        base: EntryBase {
                            id: EntryId::generate(),
                            parent_id: get_leaf(conn, session_id),
                            timestamp: Utc::now(),
                        },
                        message: tool_result_msg,
                    };
                    store::append_entry(conn, session_id, &tr_entry)?;
                    continue;
                }
                // If the hook returned mutated input, use it
                if let Some(payload) = hook_result.payload {
                    args = payload;
                }
            }

            let _ = event_tx.send(AgentLoopEvent::ToolExecuting {
                id: tc.id.clone(),
                name: tc.name.clone(),
            });

            let tool = tools.iter().find(|t| t.name() == tc.name);
            let result = match tool {
                Some(t) => t.execute(args, tool_ctx, cancel.clone()).await,
                None => Err(bb_core::error::BbError::Tool(format!(
                    "Unknown tool: {}",
                    tc.name
                ))),
            };

            let (mut content, is_error) = match result {
                Ok(r) => (r.content, r.is_error),
                Err(e) => {
                    let msg = format!("Error: {e}");
                    (vec![ContentBlock::Text { text: msg }], true)
                }
            };

            // Fire tool_result hook — can replace content
            let tr_event = Event::ToolResult(ToolResultEvent {
                tool_call_id: tc.id.clone(),
                tool_name: tc.name.clone(),
                content: content.clone(),
                is_error,
            });
            if let Some(hook_result) = event_bus.emit(&tr_event).await {
                if let Some(new_content_json) = hook_result.content {
                    if let Ok(parsed) = serde_json::from_value::<Vec<ContentBlock>>(
                        serde_json::Value::Array(new_content_json),
                    ) {
                        content = parsed;
                    }
                }
            }

            // Extract text for the event
            let content_text = content
                .iter()
                .filter_map(|c| match c {
                    ContentBlock::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");

            let _ = event_tx.send(AgentLoopEvent::ToolResult {
                id: tc.id.clone(),
                name: tc.name.clone(),
                content: content_text,
                is_error,
            });

            // Append ToolResultMessage to session
            let tool_result_msg = AgentMessage::ToolResult(ToolResultMessage {
                tool_call_id: tc.id.clone(),
                tool_name: tc.name.clone(),
                content,
                details: None,
                is_error,
                timestamp: Utc::now().timestamp_millis(),
            });

            let tr_entry = SessionEntry::Message {
                base: EntryBase {
                    id: EntryId::generate(),
                    parent_id: get_leaf(conn, session_id),
                    timestamp: Utc::now(),
                },
                message: tool_result_msg,
            };
            store::append_entry(conn, session_id, &tr_entry)?;
        }

        let _ = event_tx.send(AgentLoopEvent::TurnEnd { turn_index });

        // Fire turn_end hook
        let _ = event_bus.emit(&Event::TurnEnd { turn_index }).await;

        // Step 8: Auto-compaction check
        let compaction_settings = CompactionSettings::default();
        let ctx_check = context::build_context(conn, session_id)?;
        let total_tokens: u64 = ctx_check.messages.iter()
            .map(|m| compaction::estimate_tokens_text(&serde_json::to_string(m).unwrap_or_default()))
            .sum();

        if compaction::should_compact(total_tokens, model.context_window, &compaction_settings) {
            let path = tree::active_path(conn, session_id)?;
            if let Some(prep) = compaction::prepare_compaction(&path, &compaction_settings) {
                // Fire session_before_compact hook — can cancel
                let compact_event = Event::SessionBeforeCompact(bb_hooks::CompactPrep {
                    first_kept_entry_id: prep.first_kept_entry_id.clone(),
                    tokens_before: prep.tokens_before,
                });
                let should_skip = if let Some(hook_result) = event_bus.emit(&compact_event).await {
                    hook_result.cancel == Some(true)
                } else {
                    false
                };

                if should_skip {
                    turn_index += 1;
                    continue;
                }

                let cancel_compact = CancellationToken::new();
                let result = compaction::compact(
                    &prep, provider, &model.id, api_key, base_url,
                    None, cancel_compact,
                ).await?;

                let comp_entry = SessionEntry::Compaction {
                    base: EntryBase {
                        id: EntryId::generate(),
                        parent_id: get_leaf(conn, session_id),
                        timestamp: Utc::now(),
                    },
                    summary: result.summary,
                    first_kept_entry_id: EntryId(result.first_kept_entry_id),
                    tokens_before: result.tokens_before,
                    details: Some(serde_json::json!({
                        "readFiles": result.read_files,
                        "modifiedFiles": result.modified_files,
                    })),
                    from_plugin: false,
                };
                store::append_entry(conn, session_id, &comp_entry)?;

                let _ = event_tx.send(AgentLoopEvent::Error {
                    message: format!("[c] Context compacted ({} tokens summarized)", result.tokens_before),
                });
            }
        }

        turn_index += 1;
    }

    Ok(())
}

fn get_leaf(conn: &rusqlite::Connection, session_id: &str) -> Option<EntryId> {
    store::get_session(conn, session_id)
        .ok()
        .flatten()
        .and_then(|s| s.leaf_id.map(EntryId))
}

/// Check if the error message indicates a context overflow.
pub fn is_context_overflow(msg: &str) -> bool {
    let msg_lower = msg.to_lowercase();
    msg_lower.contains("context_length_exceeded")
        || msg_lower.contains("maximum context length")
        || msg_lower.contains("too many tokens")
        || msg_lower.contains("request too large")
        || msg_lower.contains("prompt is too long")
        || (msg_lower.contains("400") && msg_lower.contains("token"))
}

/// Check if the error message indicates a rate limit.
pub fn is_rate_limited(msg: &str) -> bool {
    msg.contains("429") || msg.to_lowercase().contains("rate limit")
}

/// Convert agent messages to provider format (JSON).
pub fn messages_to_provider(messages: &[AgentMessage]) -> Vec<serde_json::Value> {
    messages
        .iter()
        .filter_map(|msg| match msg {
            AgentMessage::User(u) => {
                let text = u
                    .content
                    .iter()
                    .filter_map(|c| match c {
                        ContentBlock::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                Some(serde_json::json!({"role": "user", "content": text}))
            }
            AgentMessage::Assistant(a) => {
                let text = agent::extract_text(&a.content);
                let tool_calls: Vec<serde_json::Value> = a
                    .content
                    .iter()
                    .filter_map(|c| match c {
                        AssistantContent::ToolCall { id, name, arguments } => {
                            Some(serde_json::json!({
                                "id": id, "type": "function",
                                "function": { "name": name, "arguments": serde_json::to_string(arguments).unwrap_or_default() }
                            }))
                        }
                        _ => None,
                    })
                    .collect();
                let mut msg = serde_json::json!({"role": "assistant"});
                if !text.is_empty() {
                    msg["content"] = serde_json::json!(text);
                }
                if !tool_calls.is_empty() {
                    msg["tool_calls"] = serde_json::json!(tool_calls);
                }
                Some(msg)
            }
            AgentMessage::ToolResult(t) => {
                let text = t
                    .content
                    .iter()
                    .filter_map(|c| match c {
                        ContentBlock::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                Some(serde_json::json!({"role": "tool", "tool_call_id": t.tool_call_id, "content": text}))
            }
            AgentMessage::CompactionSummary(c) => Some(serde_json::json!({
                "role": "user",
                "content": format!("[Previous conversation summary]\n\n{}", c.summary)
            })),
            AgentMessage::BranchSummary(b) => Some(serde_json::json!({
                "role": "user",
                "content": format!("[Branch summary]\n\n{}", b.summary)
            })),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::MessageQueue;

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

// ── Message queue (for future use) ─────────────────────────────────

#[derive(Debug, Default)]
pub struct MessageQueue {
    steers: Vec<String>,
    follow_ups: Vec<String>,
}

impl MessageQueue {
    pub fn new() -> Self { Self::default() }
    pub fn push_steer(&mut self, text: String) { self.steers.push(text); }
    pub fn push_follow_up(&mut self, text: String) { self.follow_ups.push(text); }
    pub fn take_steers(&mut self) -> Vec<String> { std::mem::take(&mut self.steers) }
    pub fn take_follow_ups(&mut self) -> Vec<String> { std::mem::take(&mut self.follow_ups) }
    pub fn is_empty(&self) -> bool { self.steers.is_empty() && self.follow_ups.is_empty() }
}
