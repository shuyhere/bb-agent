use crate::agent;
use crate::types::{AgentMessage, AssistantContent, ContentBlock, StopReason};
use serde_json::{Value, json};
use std::collections::HashSet;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct TranscriptRepairSummary {
    pub synthetic_tool_results: usize,
    pub dropped_orphan_tool_results: usize,
    pub dropped_duplicate_tool_results: usize,
    pub skipped_errored_assistant_tool_messages: usize,
}

#[derive(Clone, Debug)]
struct PendingToolCall {
    full_id: String,
    base_id: String,
    name: String,
}

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Debug)]
pub(super) struct RepairedProviderTranscript {
    pub messages: Vec<Value>,
    pub summary: TranscriptRepairSummary,
}

pub(super) fn validate_and_repair_messages_for_provider(
    messages: &[AgentMessage],
) -> RepairedProviderTranscript {
    let mut provider_messages = Vec::new();
    let mut pending_tool_calls: Vec<PendingToolCall> = Vec::new();
    let mut seen_tool_results: HashSet<String> = HashSet::new();
    let mut summary = TranscriptRepairSummary::default();

    let flush_pending_tool_results =
        |provider_messages: &mut Vec<Value>,
         pending_tool_calls: &mut Vec<PendingToolCall>,
         seen_tool_results: &mut HashSet<String>,
         summary: &mut TranscriptRepairSummary| {
            for tool_call in pending_tool_calls.iter() {
                if !seen_tool_results.contains(&tool_call.base_id) {
                    provider_messages.push(json!({
                        "role": "tool",
                        "tool_call_id": tool_call.full_id,
                        "content": "Error: tool execution interrupted before a result was recorded",
                        "synthetic": true,
                        "tool_name": tool_call.name,
                    }));
                    summary.synthetic_tool_results += 1;
                }
            }
            pending_tool_calls.clear();
            seen_tool_results.clear();
        };

    for message in messages {
        match message {
            AgentMessage::User(user) => {
                flush_pending_tool_results(
                    &mut provider_messages,
                    &mut pending_tool_calls,
                    &mut seen_tool_results,
                    &mut summary,
                );
                provider_messages.push(provider_user_message(&user.content));
            }
            AgentMessage::Assistant(assistant) => {
                if matches!(
                    assistant.stop_reason,
                    StopReason::Error | StopReason::Aborted
                ) {
                    pending_tool_calls.clear();
                    seen_tool_results.clear();
                    summary.skipped_errored_assistant_tool_messages += 1;
                    continue;
                }

                flush_pending_tool_results(
                    &mut provider_messages,
                    &mut pending_tool_calls,
                    &mut seen_tool_results,
                    &mut summary,
                );

                let text = agent::extract_text(&assistant.content);
                let tool_calls: Vec<Value> = assistant
                    .content
                    .iter()
                    .filter_map(|block| match block {
                        AssistantContent::ToolCall {
                            id,
                            name,
                            arguments,
                        } => Some(json!({
                            "id": id,
                            "type": "function",
                            "function": {
                                "name": name,
                                "arguments": serde_json::to_string(arguments).unwrap_or_default()
                            }
                        })),
                        _ => None,
                    })
                    .collect();

                pending_tool_calls = assistant
                    .content
                    .iter()
                    .filter_map(|block| match block {
                        AssistantContent::ToolCall { id, name, .. } => Some(PendingToolCall {
                            full_id: id.clone(),
                            base_id: base_tool_call_id(id),
                            name: name.clone(),
                        }),
                        _ => None,
                    })
                    .collect();
                seen_tool_results.clear();

                let mut provider_message = json!({"role": "assistant"});
                if !text.is_empty() {
                    provider_message["content"] = json!(text);
                }
                if !tool_calls.is_empty() {
                    provider_message["tool_calls"] = json!(tool_calls);
                }
                provider_messages.push(provider_message);
            }
            AgentMessage::ToolResult(tool_result) => {
                let base_id = base_tool_call_id(&tool_result.tool_call_id);
                let Some(pending) = pending_tool_calls
                    .iter()
                    .find(|tool_call| tool_call.base_id == base_id)
                else {
                    summary.dropped_orphan_tool_results += 1;
                    continue;
                };

                if !seen_tool_results.insert(base_id.clone()) {
                    summary.dropped_duplicate_tool_results += 1;
                    continue;
                }

                provider_messages.push(json!({
                    "role": "tool",
                    "tool_call_id": pending.full_id,
                    "content": provider_tool_result_content(&tool_result.content),
                }));
            }
            AgentMessage::Custom(custom) => {
                flush_pending_tool_results(
                    &mut provider_messages,
                    &mut pending_tool_calls,
                    &mut seen_tool_results,
                    &mut summary,
                );
                let text = custom
                    .content
                    .iter()
                    .filter_map(|block| match block {
                        ContentBlock::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                if !text.is_empty() {
                    provider_messages.push(json!({"role": "user", "content": text}));
                }
            }
            AgentMessage::CompactionSummary(compaction) => {
                flush_pending_tool_results(
                    &mut provider_messages,
                    &mut pending_tool_calls,
                    &mut seen_tool_results,
                    &mut summary,
                );
                provider_messages.push(json!({
                    "role": "user",
                    "content": format!("[Previous conversation summary]\n\n{}", compaction.summary),
                }));
            }
            AgentMessage::BranchSummary(branch) => {
                flush_pending_tool_results(
                    &mut provider_messages,
                    &mut pending_tool_calls,
                    &mut seen_tool_results,
                    &mut summary,
                );
                provider_messages.push(json!({
                    "role": "user",
                    "content": format!("[Branch summary]\n\n{}", branch.summary),
                }));
            }
            _ => {}
        }
    }

    flush_pending_tool_results(
        &mut provider_messages,
        &mut pending_tool_calls,
        &mut seen_tool_results,
        &mut summary,
    );

    RepairedProviderTranscript {
        messages: provider_messages,
        summary,
    }
}

fn base_tool_call_id(id: &str) -> String {
    id.split('|').next().unwrap_or(id).to_string()
}

fn provider_user_message(content: &[ContentBlock]) -> Value {
    let has_images = content
        .iter()
        .any(|block| matches!(block, ContentBlock::Image { .. }));

    if has_images {
        json!({
            "role": "user",
            "content": content_blocks_to_provider(content),
        })
    } else {
        let text = content
            .iter()
            .filter_map(|block| match block {
                ContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        json!({"role": "user", "content": text})
    }
}

fn provider_tool_result_content(content: &[ContentBlock]) -> Value {
    let has_images = content
        .iter()
        .any(|block| matches!(block, ContentBlock::Image { .. }));

    if has_images {
        json!(content_blocks_to_provider(content))
    } else {
        json!(
            content
                .iter()
                .filter_map(|block| match block {
                    ContentBlock::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n")
        )
    }
}

fn content_blocks_to_provider(content: &[ContentBlock]) -> Vec<Value> {
    content
        .iter()
        .map(|block| match block {
            ContentBlock::Text { text } => json!({
                "type": "text",
                "text": text
            }),
            ContentBlock::Image { data, mime_type } => {
                json!({
                    "type": "image",
                    "source": {
                        "type": "base64",
                        "media_type": mime_type,
                        "data": data
                    }
                })
            }
        })
        .collect()
}
