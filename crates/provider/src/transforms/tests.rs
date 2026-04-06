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
    assert!(
        normalized
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    );
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
    assert_eq!(result.len(), 0);
}

#[test]
fn test_anthropic_user_image_blocks_pass_through() {
    let messages = vec![json!({
        "role": "user",
        "content": [
            { "type": "text", "text": "What is in this image?" },
            { "type": "image", "source": {
                "type": "base64",
                "media_type": "image/png",
                "data": "iVBORw0KGgo="
            }}
        ]
    })];
    let result = convert_messages_for_anthropic(&messages);
    assert_eq!(result.len(), 1);
    let content = result[0]["content"].as_array().unwrap();
    assert_eq!(content.len(), 2);
    assert_eq!(content[0]["type"], "text");
    assert_eq!(content[1]["type"], "image");
    assert_eq!(content[1]["source"]["media_type"], "image/png");
}

#[test]
fn test_openai_converts_image_blocks_to_image_url() {
    let messages = vec![json!({
        "role": "user",
        "content": [
            { "type": "text", "text": "What is this?" },
            { "type": "image", "source": {
                "type": "base64",
                "media_type": "image/jpeg",
                "data": "/9j/4AAQ"
            }}
        ]
    })];
    let result = convert_messages_for_openai(&messages);
    assert_eq!(result.len(), 1);
    let content = result[0]["content"].as_array().unwrap();
    assert_eq!(content.len(), 2);
    assert_eq!(content[0]["type"], "text");
    assert_eq!(content[1]["type"], "image_url");
    let url = content[1]["image_url"]["url"].as_str().unwrap();
    assert!(url.starts_with("data:image/jpeg;base64,"));
    assert!(url.contains("/9j/4AAQ"));
}

#[test]
fn test_openai_text_only_still_flattened() {
    let messages = vec![json!({
        "role": "user",
        "content": [
            { "type": "text", "text": "Hello world" }
        ]
    })];
    let result = convert_messages_for_openai(&messages);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0]["content"], "Hello world");
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
