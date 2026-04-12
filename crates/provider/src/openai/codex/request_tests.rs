use super::request::{convert_messages_for_codex, sanitize_messages_for_codex};
use serde_json::json;

#[test]
fn sanitize_messages_for_codex_drops_orphan_tool_results() {
    let messages = vec![
        json!({
            "role": "assistant",
            "tool_calls": [{
                "id": "call_a|item_a",
                "type": "function",
                "function": {"name": "read", "arguments": "{}"}
            }]
        }),
        json!({
            "role": "tool",
            "tool_call_id": "call_a|item_a",
            "content": "ok"
        }),
        json!({
            "role": "tool",
            "tool_call_id": "call_missing|item_x",
            "content": "orphan"
        }),
        json!({"role": "user", "content": "next"}),
    ];

    let sanitized = sanitize_messages_for_codex(&messages);
    assert_eq!(sanitized.len(), 3);
    assert_eq!(sanitized[1]["tool_call_id"], "call_a|item_a");
}

#[test]
fn convert_messages_for_codex_preserves_user_image_blocks() {
    let messages = vec![json!({
        "role": "user",
        "content": [
            {"type": "text", "text": "describe this"},
            {
                "type": "image",
                "source": {
                    "type": "base64",
                    "media_type": "image/png",
                    "data": "abcd"
                }
            }
        ]
    })];

    let converted = convert_messages_for_codex(&messages);
    assert_eq!(converted.len(), 1);
    let content = converted[0]["content"].as_array().expect("content array");
    assert_eq!(content[0]["type"], "input_text");
    assert_eq!(content[1]["type"], "input_image");
    assert_eq!(content[1]["detail"], "high");
    assert_eq!(content[1]["image_url"], "data:image/png;base64,abcd");
}
