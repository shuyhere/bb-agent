//! Message transform helpers for cross-provider compatibility.
//!
//! Handles converting between
//! Anthropic and OpenAI message formats, normalizing tool call IDs, and
//! stripping thinking blocks for providers that don't support them.

use serde_json::{Value, json};

#[cfg(test)]
mod tests;

/// Ensure messages use Anthropic format before sending to the Anthropic API.
///
/// - Converts OpenAI-style `tool_calls` on assistant messages into `tool_use` content blocks.
/// - Converts `role: "tool"` messages into `role: "user"` with `tool_result` content blocks.
/// - Normalizes tool call IDs to match Anthropic's `^[a-zA-Z0-9_-]+$` (max 64 chars).
/// - Converts image content blocks to Anthropic's `{ type: "image", source: { type: "base64", ... } }` format.
/// - Passes through user/assistant messages with content arrays unchanged.
pub fn convert_messages_for_anthropic(messages: &[Value]) -> Vec<Value> {
    messages
        .iter()
        .filter_map(|msg| {
            let role = msg["role"].as_str()?;
            match role {
                "user" => {
                    // Pass through content — may be a string or array of blocks.
                    // Image blocks from messages_to_provider already use Anthropic format:
                    //   { type: "image", source: { type: "base64", media_type, data } }
                    Some(json!({
                        "role": "user",
                        "content": msg["content"],
                    }))
                }
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
                            let id = normalize_tool_call_id(tc["id"].as_str().unwrap_or(""));
                            let args_str = tc["function"]["arguments"].as_str().unwrap_or("{}");
                            let args: Value = serde_json::from_str(args_str).unwrap_or(json!({}));
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
                    let tool_call_id =
                        normalize_tool_call_id(msg["tool_call_id"].as_str().unwrap_or(""));
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
                        // Regular user message – convert image blocks to OpenAI format
                        let converted = convert_content_blocks_for_openai(arr);
                        result.push(json!({
                            "role": "user",
                            "content": converted,
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
                                if let Some(t) = block["text"].as_str()
                                    && !t.is_empty()
                                {
                                    text_parts.push(t.to_string());
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

/// Convert user content blocks from Anthropic/internal format to OpenAI format.
///
/// - Text blocks: `{ type: "text", text }` (same)
/// - Image blocks: `{ type: "image", source: { ... } }` → `{ type: "image_url", image_url: { url: "data:..." } }`
/// - If only text and a single block, flatten to plain string.
fn convert_content_blocks_for_openai(arr: &[Value]) -> Value {
    let has_images = arr.iter().any(|b| b["type"].as_str() == Some("image"));

    if !has_images {
        return flatten_content_array(arr);
    }

    // Convert each block
    let blocks: Vec<Value> = arr
        .iter()
        .map(|block| {
            let btype = block["type"].as_str().unwrap_or("");
            match btype {
                "text" => json!({
                    "type": "text",
                    "text": block["text"]
                }),
                "image" => {
                    // Anthropic format: { source: { type: "base64", media_type, data } }
                    let media_type = block["source"]["media_type"]
                        .as_str()
                        .unwrap_or("image/png");
                    let data = block["source"]["data"].as_str().unwrap_or("");
                    json!({
                        "type": "image_url",
                        "image_url": {
                            "url": format!("data:{media_type};base64,{data}")
                        }
                    })
                }
                _ => block.clone(),
            }
        })
        .collect();

    json!(blocks)
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
                            if let Some(text) = block["thinking"].as_str()
                                && !text.trim().is_empty()
                            {
                                return Some(json!({
                                    "type": "text",
                                    "text": format!("[Thinking]\n{text}"),
                                }));
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
