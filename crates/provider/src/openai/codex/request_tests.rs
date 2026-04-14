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

#[test]
fn convert_messages_for_codex_flattens_structured_tool_output_blocks() {
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
            "content": [
                {"type": "text", "text": "first line"},
                {
                    "type": "image",
                    "source": {"media_type": "image/png", "data": "abcd"}
                },
                {"type": "text", "text": "second line"}
            ]
        }),
    ];

    let converted = convert_messages_for_codex(&messages);
    assert_eq!(converted.len(), 2);
    assert_eq!(converted[1]["type"], "function_call_output");
    assert_eq!(converted[1]["call_id"], "call_a");
    assert_eq!(
        converted[1]["output"],
        "first line\n[tool returned image result: image/png]\nsecond line"
    );
}

#[test]
fn sanitize_messages_for_codex_resets_pending_tool_calls_after_user_turn() {
    let messages = vec![
        json!({
            "role": "assistant",
            "tool_calls": [{
                "id": "call_a|item_a",
                "type": "function",
                "function": {"name": "read", "arguments": "{}"}
            }]
        }),
        json!({"role": "user", "content": "continue"}),
        json!({
            "role": "tool",
            "tool_call_id": "call_a|item_a",
            "content": "stale tool output"
        }),
    ];

    let sanitized = sanitize_messages_for_codex(&messages);
    assert_eq!(sanitized.len(), 2);
    assert_eq!(sanitized[1]["role"], "user");
}
