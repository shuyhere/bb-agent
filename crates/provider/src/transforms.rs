//! Message transform helpers for cross-provider compatibility.
//!
//! Ported from pi's `transform-messages.ts`. Handles converting between
//! Anthropic and OpenAI message formats, normalizing tool call IDs, and
//! stripping thinking blocks for providers that don't support them.

use serde_json::{json, Value};

/// Ensure messages use Anthropic format before sending to the Anthropic API.
///
/// - Converts OpenAI-style `tool_calls` on assistant messages into `tool_use` content blocks.
/// - Converts `role: "tool"` messages into `role: "user"` with `tool_result` content blocks.
/// - Normalizes tool call IDs to match Anthropic's `^[a-zA-Z0-9_-]+$` (max 64 chars).
/// - Passes through user/assistant messages with content arrays unchanged.
pub fn convert_messages_for_anthropic(messages: &[Value]) -> Vec<Value> {
    messages
        .iter()
        .filter_map(|msg| {
            let role = msg["role"].as_str()?;
            match role {
                "user" => Some(json!({
                    "role": "user",
                    "content": msg["content"],
                })),
                "assistant" => {
                    let mut content = Vec::new();

                    // If content is already an array (native Anthropic format), use it
                    if let Some(arr) = msg["content"].as_array() {
                        for block in arr {
                            let btype = block["type"].as_str().unwrap_or("");
                            match btype {
                                "thinking" | "text" | "tool_use" => {
                                    content.push(block.clone());
                                }
                                _ => {
                                    content.push(block.clone());
                                }
                            }
                        }
                    } else if let Some(text) = msg["content"].as_str() {
                        // Simple text string
                        if !text.is_empty() {
                            content.push(json!({"type": "text", "text": text}));
                        }
                    }

                    // OpenAI-style tool_calls → Anthropic tool_use blocks
                    if let Some(tool_calls) = msg["tool_calls"].as_array() {
                        for tc in tool_calls {
                            let id = normalize_tool_call_id(
                                tc["id"].as_str().unwrap_or(""),
                            );
                            let args_str = tc["function"]["arguments"].as_str().unwrap_or("{}");
                            let args: Value =
                                serde_json::from_str(args_str).unwrap_or(json!({}));
                            content.push(json!({
                                "type": "tool_use",
                                "id": id,
                                "name": tc["function"]["name"],
                                "input": args,
                            }));
                        }
                    }

                    if content.is_empty() {
                        return None;
                    }

                    Some(json!({
                        "role": "assistant",
                        "content": content,
                    }))
                }
                "tool" => {
                    let tool_call_id = normalize_tool_call_id(
                        msg["tool_call_id"].as_str().unwrap_or(""),
                    );
                    Some(json!({
                        "role": "user",
                        "content": [{
                            "type": "tool_result",
                            "tool_use_id": tool_call_id,
                            "content": msg["content"],
                        }],
                    }))
                }
                "system" => None, // handled separately
                _ => None,
            }
        })
        .collect()
}

/// Ensure messages use OpenAI format before sending to OpenAI-compatible APIs.
///
/// - Converts Anthropic-style `tool_use` content blocks into OpenAI `tool_calls`.
/// - Converts `tool_result` content blocks into `role: "tool"` messages.
/// - Flattens content arrays with a single text block into a plain string.
pub fn convert_messages_for_openai(messages: &[Value]) -> Vec<Value> {
    let mut result = Vec::new();

    for msg in messages {
        let role = match msg["role"].as_str() {
            Some(r) => r,
            None => continue,
        };

        match role {
            "user" => {
                // Check if content contains tool_result blocks (Anthropic format)
                if let Some(arr) = msg["content"].as_array() {
                    let has_tool_results = arr
                        .iter()
                        .any(|b| b["type"].as_str() == Some("tool_result"));

                    if has_tool_results {
                        // Convert each tool_result into a separate "tool" message
                        for block in arr {
                            if block["type"].as_str() == Some("tool_result") {
                                result.push(json!({
                                    "role": "tool",
                                    "tool_call_id": block["tool_use_id"],
                                    "content": flatten_content(&block["content"]),
                                }));
                            }
                        }
                    } else {
                        // Regular user message – flatten if single text block
                        result.push(json!({
                            "role": "user",
                            "content": flatten_content_array(arr),
                        }));
                    }
                } else {
                    result.push(json!({
                        "role": "user",
                        "content": msg["content"],
                    }));
                }
            }
            "assistant" => {
                let mut text_parts = Vec::new();
                let mut tool_calls = Vec::new();

                if let Some(arr) = msg["content"].as_array() {
                    for block in arr {
                        let btype = block["type"].as_str().unwrap_or("");
                        match btype {
                            "text" => {
                                if let Some(t) = block["text"].as_str() {
                                    if !t.is_empty() {
                                        text_parts.push(t.to_string());
                                    }
                                }
                            }
                            "tool_use" => {
                                let args = if block["input"].is_object() {
                                    serde_json::to_string(&block["input"])
                                        .unwrap_or_else(|_| "{}".to_string())
                                } else {
                                    "{}".to_string()
                                };
                                tool_calls.push(json!({
                                    "id": block["id"],
                                    "type": "function",
                                    "function": {
                                        "name": block["name"],
                                        "arguments": args,
                                    }
                                }));
                            }
                            // thinking blocks are skipped for OpenAI (handled by strip_thinking_blocks)
                            _ => {}
                        }
                    }
                } else if let Some(text) = msg["content"].as_str() {
                    text_parts.push(text.to_string());
                }

                // Also handle pre-existing OpenAI-style tool_calls
                if let Some(existing_tcs) = msg["tool_calls"].as_array() {
                    tool_calls.extend(existing_tcs.iter().cloned());
                }

                let mut out = json!({"role": "assistant"});
                let combined_text = text_parts.join("");
                if !combined_text.is_empty() {
                    out["content"] = json!(combined_text);
                } else {
                    out["content"] = Value::Null;
                }
                if !tool_calls.is_empty() {
                    out["tool_calls"] = json!(tool_calls);
                }

                result.push(out);
            }
            "tool" => {
                // Already in OpenAI format
                result.push(msg.clone());
            }
            "system" => {
                result.push(msg.clone());
            }
            _ => {
                result.push(msg.clone());
            }
        }
    }

    result
}

/// Remove thinking blocks from messages for providers that don't support them.
///
/// - Removes `type: "thinking"` content blocks from assistant messages.
/// - Optionally converts non-empty thinking text into a text block prefixed with `[Thinking]`.
/// - If an assistant message becomes empty after stripping, it is dropped entirely.
pub fn strip_thinking_blocks(messages: &[Value]) -> Vec<Value> {
    messages
        .iter()
        .filter_map(|msg| {
            let role = msg["role"].as_str().unwrap_or("");
            if role != "assistant" {
                return Some(msg.clone());
            }

            // Only process array content
            if let Some(arr) = msg["content"].as_array() {
                let filtered: Vec<Value> = arr
                    .iter()
                    .filter_map(|block| {
                        let btype = block["type"].as_str().unwrap_or("");
                        if btype == "thinking" {
                            // Convert non-empty thinking to text for context preservation
                            if let Some(text) = block["thinking"].as_str() {
                                if !text.trim().is_empty() {
                                    return Some(json!({
                                        "type": "text",
                                        "text": format!("[Thinking]\n{text}"),
                                    }));
                                }
                            }
                            None
                        } else {
                            Some(block.clone())
                        }
                    })
                    .collect();

                if filtered.is_empty() {
                    return None;
                }

                let mut out = msg.clone();
                out["content"] = json!(filtered);
                Some(out)
            } else {
                Some(msg.clone())
            }
        })
        .collect()
}

/// Normalize a tool call ID to match Anthropic's constraints:
/// `^[a-zA-Z0-9_-]+$`, max 64 chars.
///
/// OpenAI Responses API generates IDs that are 450+ chars with special chars like `|`.
fn normalize_tool_call_id(id: &str) -> String {
    if id.is_empty() {
        return "tool_0".to_string();
    }

    // Check if already valid
    let is_valid = id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if is_valid {
        return id.to_string();
    }

    // Sanitize: replace invalid chars with underscore, truncate to 64
    let sanitized: String = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .take(64)
        .collect();

    if sanitized.is_empty() {
        "tool_0".to_string()
    } else {
        sanitized
    }
}

/// Flatten a content Value — if it's a string return it, if it's an array of
/// blocks join text blocks.
fn flatten_content(content: &Value) -> Value {
    if content.is_string() {
        return content.clone();
    }
    if let Some(arr) = content.as_array() {
        let text: String = arr
            .iter()
            .filter_map(|b| {
                if b["type"].as_str() == Some("text") {
                    b["text"].as_str().map(|s| s.to_string())
                } else {
                    b.as_str().map(|s| s.to_string())
                }
            })
            .collect::<Vec<_>>()
            .join("");
        if text.is_empty() {
            content.clone()
        } else {
            json!(text)
        }
    } else {
        content.clone()
    }
}

/// Flatten an array of content blocks. If there's only a single text block,
/// return the text as a plain string.
fn flatten_content_array(arr: &[Value]) -> Value {
    if arr.len() == 1 {
        if let Some(text) = arr[0]["text"].as_str() {
            return json!(text);
        }
        if arr[0].is_string() {
            return arr[0].clone();
        }
    }
    json!(arr)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_tool_call_id_valid() {
        assert_eq!(normalize_tool_call_id("toolu_abc123"), "toolu_abc123");
        assert_eq!(normalize_tool_call_id("call-xyz"), "call-xyz");
    }

    #[test]
    fn test_normalize_tool_call_id_special_chars() {
        let long_id = "call_abc|def|ghi";
        let normalized = normalize_tool_call_id(long_id);
        assert!(normalized.len() <= 64);
        assert!(normalized
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'));
    }

    #[test]
    fn test_normalize_tool_call_id_too_long() {
        let long_id = "a".repeat(200);
        let normalized = normalize_tool_call_id(&long_id);
        assert_eq!(normalized.len(), 64);
    }

    #[test]
    fn test_normalize_tool_call_id_empty() {
        assert_eq!(normalize_tool_call_id(""), "tool_0");
    }

    #[test]
    fn test_convert_messages_for_anthropic_tool_calls() {
        let messages = vec![json!({
            "role": "assistant",
            "content": "Let me search for that.",
            "tool_calls": [{
                "id": "call_123",
                "function": {
                    "name": "search",
                    "arguments": "{\"query\": \"test\"}"
                }
            }]
        })];
        let result = convert_messages_for_anthropic(&messages);
        assert_eq!(result.len(), 1);
        let content = result[0]["content"].as_array().unwrap();
        assert_eq!(content.len(), 2);
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[1]["type"], "tool_use");
        assert_eq!(content[1]["id"], "call_123");
    }

    #[test]
    fn test_convert_messages_for_anthropic_tool_result() {
        let messages = vec![json!({
            "role": "tool",
            "tool_call_id": "call_123",
            "content": "search result here"
        })];
        let result = convert_messages_for_anthropic(&messages);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0]["role"], "user");
        let content = result[0]["content"].as_array().unwrap();
        assert_eq!(content[0]["type"], "tool_result");
        assert_eq!(content[0]["tool_use_id"], "call_123");
    }

    #[test]
    fn test_convert_messages_for_openai_tool_use() {
        let messages = vec![json!({
            "role": "assistant",
            "content": [
                {"type": "text", "text": "Let me check."},
                {"type": "tool_use", "id": "toolu_abc", "name": "read", "input": {"path": "/tmp"}}
            ]
        })];
        let result = convert_messages_for_openai(&messages);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0]["role"], "assistant");
        assert_eq!(result[0]["content"], "Let me check.");
        let tcs = result[0]["tool_calls"].as_array().unwrap();
        assert_eq!(tcs.len(), 1);
        assert_eq!(tcs[0]["id"], "toolu_abc");
        assert_eq!(tcs[0]["function"]["name"], "read");
    }

    #[test]
    fn test_convert_messages_for_openai_tool_result() {
        let messages = vec![json!({
            "role": "user",
            "content": [{
                "type": "tool_result",
                "tool_use_id": "toolu_abc",
                "content": "file contents here"
            }]
        })];
        let result = convert_messages_for_openai(&messages);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0]["role"], "tool");
        assert_eq!(result[0]["tool_call_id"], "toolu_abc");
    }

    #[test]
    fn test_strip_thinking_blocks() {
        let messages = vec![json!({
            "role": "assistant",
            "content": [
                {"type": "thinking", "thinking": "Let me reason about this..."},
                {"type": "text", "text": "Here is my answer."}
            ]
        })];
        let result = strip_thinking_blocks(&messages);
        assert_eq!(result.len(), 1);
        let content = result[0]["content"].as_array().unwrap();
        // thinking was converted to text
        assert_eq!(content.len(), 2);
        assert_eq!(content[0]["type"], "text");
        assert!(content[0]["text"].as_str().unwrap().contains("[Thinking]"));
        assert_eq!(content[1]["type"], "text");
    }

    #[test]
    fn test_strip_thinking_blocks_empty() {
        let messages = vec![json!({
            "role": "assistant",
            "content": [
                {"type": "thinking", "thinking": ""}
            ]
        })];
        let result = strip_thinking_blocks(&messages);
        // Empty thinking stripped, empty message dropped
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_strip_thinking_preserves_non_assistant() {
        let messages = vec![
            json!({"role": "user", "content": "Hello"}),
            json!({"role": "assistant", "content": [
                {"type": "text", "text": "Hi"}
            ]}),
        ];
        let result = strip_thinking_blocks(&messages);
        assert_eq!(result.len(), 2);
    }
}
