use crate::agent;
use serde_json::json;

pub fn messages_to_provider(messages: &[crate::types::AgentMessage]) -> Vec<serde_json::Value> {
    use crate::types::{AgentMessage, AssistantContent, ContentBlock, StopReason};
    use std::collections::HashSet;

    let mut provider_messages = Vec::new();
    let mut pending_tool_calls: Vec<(String, String)> = Vec::new();
    let mut seen_tool_results: HashSet<String> = HashSet::new();

    let flush_pending_tool_results =
        |provider_messages: &mut Vec<serde_json::Value>,
         pending_tool_calls: &mut Vec<(String, String)>,
         seen_tool_results: &mut HashSet<String>| {
            for (id, name) in pending_tool_calls.iter() {
                let base_id = id.split('|').next().unwrap_or(id.as_str()).to_string();
                if !seen_tool_results.contains(&base_id) {
                    provider_messages.push(json!({
                        "role": "tool",
                        "tool_call_id": id,
                        "content": "Error: tool execution interrupted before a result was recorded",
                        "synthetic": true,
                        "tool_name": name,
                    }));
                }
            }
            pending_tool_calls.clear();
            seen_tool_results.clear();
        };

    for msg in messages {
        match msg {
            AgentMessage::User(user) => {
                flush_pending_tool_results(
                    &mut provider_messages,
                    &mut pending_tool_calls,
                    &mut seen_tool_results,
                );
                let has_images = user
                    .content
                    .iter()
                    .any(|block| matches!(block, ContentBlock::Image { .. }));

                if has_images {
                    provider_messages.push(json!({
                        "role": "user",
                        "content": content_blocks_to_provider(&user.content),
                    }));
                } else {
                    let text = user
                        .content
                        .iter()
                        .filter_map(|block| match block {
                            ContentBlock::Text { text } => Some(text.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    provider_messages.push(serde_json::json!({"role": "user", "content": text}));
                }
            }
            AgentMessage::Assistant(assistant) => {
                if matches!(
                    assistant.stop_reason,
                    StopReason::Error | StopReason::Aborted
                ) {
                    pending_tool_calls.clear();
                    seen_tool_results.clear();
                    continue;
                }

                flush_pending_tool_results(
                    &mut provider_messages,
                    &mut pending_tool_calls,
                    &mut seen_tool_results,
                );

                let text = agent::extract_text(&assistant.content);
                let tool_calls: Vec<serde_json::Value> = assistant
                    .content
                    .iter()
                    .filter_map(|block| match block {
                        AssistantContent::ToolCall {
                            id,
                            name,
                            arguments,
                        } => Some(serde_json::json!({
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
                        AssistantContent::ToolCall { id, name, .. } => {
                            Some((id.clone(), name.clone()))
                        }
                        _ => None,
                    })
                    .collect();
                seen_tool_results.clear();

                let mut msg = serde_json::json!({"role": "assistant"});
                if !text.is_empty() {
                    msg["content"] = serde_json::json!(text);
                }
                if !tool_calls.is_empty() {
                    msg["tool_calls"] = serde_json::json!(tool_calls);
                }
                provider_messages.push(msg);
            }
            AgentMessage::ToolResult(tool_result) => {
                let tool_call_id_base = tool_result
                    .tool_call_id
                    .split('|')
                    .next()
                    .unwrap_or(tool_result.tool_call_id.as_str())
                    .to_string();
                if !pending_tool_calls
                    .iter()
                    .any(|(id, _)| id.split('|').next().unwrap_or(id.as_str()) == tool_call_id_base)
                {
                    continue;
                }
                seen_tool_results.insert(tool_call_id_base);

                let has_images = tool_result
                    .content
                    .iter()
                    .any(|block| matches!(block, ContentBlock::Image { .. }));
                let content = if has_images {
                    json!(content_blocks_to_provider(&tool_result.content))
                } else {
                    json!(
                        tool_result
                            .content
                            .iter()
                            .filter_map(|block| match block {
                                ContentBlock::Text { text } => Some(text.as_str()),
                                _ => None,
                            })
                            .collect::<Vec<_>>()
                            .join("\n")
                    )
                };
                provider_messages.push(json!({
                    "role": "tool",
                    "tool_call_id": tool_result.tool_call_id,
                    "content": content,
                }));
            }
            AgentMessage::Custom(custom) => {
                flush_pending_tool_results(
                    &mut provider_messages,
                    &mut pending_tool_calls,
                    &mut seen_tool_results,
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
                    provider_messages.push(serde_json::json!({"role": "user", "content": text}));
                }
            }
            AgentMessage::CompactionSummary(compaction) => {
                flush_pending_tool_results(
                    &mut provider_messages,
                    &mut pending_tool_calls,
                    &mut seen_tool_results,
                );
                provider_messages.push(serde_json::json!({
                    "role": "user",
                    "content": format!("[Previous conversation summary]\n\n{}", compaction.summary),
                }));
            }
            AgentMessage::BranchSummary(branch) => {
                flush_pending_tool_results(
                    &mut provider_messages,
                    &mut pending_tool_calls,
                    &mut seen_tool_results,
                );
                provider_messages.push(serde_json::json!({
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
    );

    provider_messages
}

fn content_blocks_to_provider(content: &[crate::types::ContentBlock]) -> Vec<serde_json::Value> {
    content
        .iter()
        .map(|block| match block {
            crate::types::ContentBlock::Text { text } => json!({
                "type": "text",
                "text": text
            }),
            crate::types::ContentBlock::Image { data, mime_type } => {
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
