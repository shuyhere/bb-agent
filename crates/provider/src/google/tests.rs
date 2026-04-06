use super::*;
use serde_json::json;

#[test]
fn test_convert_user_message() {
    let messages = vec![json!({
        "role": "user",
        "content": "Hello"
    })];
    let result = convert_messages_google(&messages);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0]["role"], "user");
    assert_eq!(result[0]["parts"][0]["text"], "Hello");
}

#[test]
fn test_convert_assistant_message() {
    let messages = vec![json!({
        "role": "assistant",
        "content": "Hi there!"
    })];
    let result = convert_messages_google(&messages);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0]["role"], "model");
    assert_eq!(result[0]["parts"][0]["text"], "Hi there!");
}

#[test]
fn test_convert_assistant_with_tool_calls() {
    let messages = vec![json!({
        "role": "assistant",
        "content": "",
        "tool_calls": [{
            "id": "call_1",
            "function": {
                "name": "read",
                "arguments": "{\"path\":\"foo.rs\"}"
            }
        }]
    })];
    let result = convert_messages_google(&messages);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0]["role"], "model");
    let fc = &result[0]["parts"][0]["functionCall"];
    assert_eq!(fc["name"], "read");
    assert_eq!(fc["args"]["path"], "foo.rs");
}

#[test]
fn test_convert_tool_result() {
    let messages = vec![json!({
        "role": "tool",
        "name": "read",
        "tool_call_id": "call_1",
        "content": "file contents here"
    })];
    let result = convert_messages_google(&messages);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0]["role"], "user");
    let fr = &result[0]["parts"][0]["functionResponse"];
    assert_eq!(fr["name"], "read");
    assert_eq!(fr["response"]["content"], "file contents here");
}

#[test]
fn test_system_message_filtered() {
    let messages = vec![json!({
        "role": "system",
        "content": "You are helpful"
    })];
    let result = convert_messages_google(&messages);
    assert!(result.is_empty());
}

#[test]
fn test_convert_user_message_with_image() {
    let messages = vec![json!({
        "role": "user",
        "content": [
            { "type": "text", "text": "What is this?" },
            { "type": "image", "source": {
                "type": "base64",
                "media_type": "image/png",
                "data": "iVBORw0KGgo="
            }}
        ]
    })];
    let result = convert_messages_google(&messages);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0]["role"], "user");
    let parts = result[0]["parts"].as_array().unwrap();
    assert_eq!(parts.len(), 2);
    assert_eq!(parts[0]["text"], "What is this?");
    assert_eq!(parts[1]["inlineData"]["mimeType"], "image/png");
    assert_eq!(parts[1]["inlineData"]["data"], "iVBORw0KGgo=");
}

#[test]
fn test_convert_tools() {
    let tools = vec![json!({
        "function": {
            "name": "read",
            "description": "Read a file",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string" }
                },
                "required": ["path"]
            }
        }
    })];
    let result = convert_tools_google(&tools);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0]["name"], "read");
    assert_eq!(result[0]["description"], "Read a file");
    assert_eq!(result[0]["parameters"]["type"], "OBJECT");
    assert_eq!(
        result[0]["parameters"]["properties"]["path"]["type"],
        "STRING"
    );
}

#[test]
fn test_convert_tools_empty() {
    let tools: Vec<Value> = vec![];
    let result = convert_tools_google(&tools);
    assert!(result.is_empty());
}

#[test]
fn test_process_google_event_text() {
    let event = json!({
        "candidates": [{
            "content": {
                "parts": [{ "text": "Hello world" }],
                "role": "model"
            }
        }]
    });
    let (tx, mut rx) = mpsc::unbounded_channel();
    process_google_event(&event, &tx);
    drop(tx);

    let mut events = Vec::new();
    while let Ok(e) = rx.try_recv() {
        events.push(e);
    }
    assert!(!events.is_empty());
    match &events[0] {
        StreamEvent::TextDelta { text } => assert_eq!(text, "Hello world"),
        other => panic!("Expected TextDelta, got {:?}", other),
    }
}

#[test]
fn test_process_google_event_function_call() {
    let event = json!({
        "candidates": [{
            "content": {
                "parts": [{
                    "functionCall": {
                        "name": "read",
                        "args": { "path": "foo.rs" }
                    }
                }],
                "role": "model"
            }
        }]
    });
    let (tx, mut rx) = mpsc::unbounded_channel();
    process_google_event(&event, &tx);
    drop(tx);

    let mut events = Vec::new();
    while let Ok(e) = rx.try_recv() {
        events.push(e);
    }
    assert_eq!(events.len(), 3);
    match &events[0] {
        StreamEvent::ToolCallStart { name, .. } => assert_eq!(name, "read"),
        other => panic!("Expected ToolCallStart, got {:?}", other),
    }
}

#[test]
fn test_process_google_event_usage() {
    let event = json!({
        "usageMetadata": {
            "promptTokenCount": 100,
            "candidatesTokenCount": 50
        }
    });
    let (tx, mut rx) = mpsc::unbounded_channel();
    process_google_event(&event, &tx);
    drop(tx);

    let mut events = Vec::new();
    while let Ok(e) = rx.try_recv() {
        events.push(e);
    }
    assert_eq!(events.len(), 1);
    match &events[0] {
        StreamEvent::Usage(u) => {
            assert_eq!(u.input_tokens, 100);
            assert_eq!(u.output_tokens, 50);
        }
        other => panic!("Expected Usage, got {:?}", other),
    }
}
