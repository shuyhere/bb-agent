use super::provider::messages_to_provider;
use crate::types::{
    AgentMessage, AssistantContent, AssistantMessage, ContentBlock, StopReason, ToolResultMessage,
    Usage,
};

#[test]
fn errored_assistant_tool_calls_are_skipped_and_do_not_accept_following_tool_results() {
    let messages = vec![
        AgentMessage::Assistant(AssistantMessage {
            content: vec![AssistantContent::ToolCall {
                id: "call_1".to_string(),
                name: "read".to_string(),
                arguments: serde_json::json!({"path": "a.txt"}),
            }],
            provider: "openai".to_string(),
            model: "gpt".to_string(),
            usage: Usage::default(),
            stop_reason: StopReason::Error,
            error_message: Some("provider error".to_string()),
            timestamp: 0,
        }),
        AgentMessage::ToolResult(ToolResultMessage {
            tool_call_id: "call_1".to_string(),
            tool_name: "read".to_string(),
            content: vec![ContentBlock::Text {
                text: "should be dropped".to_string(),
            }],
            details: None,
            is_error: true,
            timestamp: 0,
        }),
    ];

    let provider_messages = messages_to_provider(&messages);
    assert!(provider_messages.is_empty());
}

#[test]
fn interrupted_tool_call_is_flushed_as_synthetic_tool_result_before_next_user_message() {
    let messages = vec![
        AgentMessage::Assistant(AssistantMessage {
            content: vec![AssistantContent::ToolCall {
                id: "call_1".to_string(),
                name: "bash".to_string(),
                arguments: serde_json::json!({"command": "pwd"}),
            }],
            provider: "openai".to_string(),
            model: "gpt".to_string(),
            usage: Usage::default(),
            stop_reason: StopReason::ToolUse,
            error_message: None,
            timestamp: 0,
        }),
        AgentMessage::User(crate::types::UserMessage {
            content: vec![ContentBlock::Text {
                text: "keep going".to_string(),
            }],
            timestamp: 1,
        }),
    ];

    let provider_messages = messages_to_provider(&messages);
    assert_eq!(provider_messages.len(), 3);
    assert_eq!(provider_messages[0]["role"], "assistant");
    assert_eq!(provider_messages[1]["role"], "tool");
    assert_eq!(provider_messages[1]["tool_call_id"], "call_1");
    assert_eq!(
        provider_messages[1]["content"],
        "Error: tool execution interrupted before a result was recorded"
    );
    assert_eq!(provider_messages[2]["role"], "user");
}

#[test]
fn interrupted_tool_call_does_not_poison_later_turns_with_real_tool_results() {
    let messages = vec![
        AgentMessage::Assistant(AssistantMessage {
            content: vec![AssistantContent::ToolCall {
                id: "call_1".to_string(),
                name: "bash".to_string(),
                arguments: serde_json::json!({"command": "pwd"}),
            }],
            provider: "openai".to_string(),
            model: "gpt".to_string(),
            usage: Usage::default(),
            stop_reason: StopReason::ToolUse,
            error_message: None,
            timestamp: 0,
        }),
        AgentMessage::User(crate::types::UserMessage {
            content: vec![ContentBlock::Text {
                text: "try again".to_string(),
            }],
            timestamp: 1,
        }),
        AgentMessage::Assistant(AssistantMessage {
            content: vec![AssistantContent::ToolCall {
                id: "call_2".to_string(),
                name: "read".to_string(),
                arguments: serde_json::json!({"path": "Cargo.toml"}),
            }],
            provider: "openai".to_string(),
            model: "gpt".to_string(),
            usage: Usage::default(),
            stop_reason: StopReason::ToolUse,
            error_message: None,
            timestamp: 2,
        }),
        AgentMessage::ToolResult(ToolResultMessage {
            tool_call_id: "call_2".to_string(),
            tool_name: "read".to_string(),
            content: vec![ContentBlock::Text {
                text: "[package]\nname = \"demo\"".to_string(),
            }],
            details: None,
            is_error: false,
            timestamp: 3,
        }),
        AgentMessage::User(crate::types::UserMessage {
            content: vec![ContentBlock::Text {
                text: "what happened?".to_string(),
            }],
            timestamp: 4,
        }),
    ];

    let provider_messages = messages_to_provider(&messages);
    assert_eq!(provider_messages.len(), 6);
    assert_eq!(provider_messages[0]["role"], "assistant");
    assert_eq!(provider_messages[0]["tool_calls"][0]["id"], "call_1");
    assert_eq!(provider_messages[1]["role"], "tool");
    assert_eq!(provider_messages[1]["tool_call_id"], "call_1");
    assert_eq!(
        provider_messages[1]["content"],
        "Error: tool execution interrupted before a result was recorded"
    );
    assert_eq!(provider_messages[2]["role"], "user");
    assert_eq!(provider_messages[2]["content"], "try again");
    assert_eq!(provider_messages[3]["role"], "assistant");
    assert_eq!(provider_messages[3]["tool_calls"][0]["id"], "call_2");
    assert_eq!(provider_messages[4]["role"], "tool");
    assert_eq!(provider_messages[4]["tool_call_id"], "call_2");
    assert_eq!(
        provider_messages[4]["content"],
        "[package]\nname = \"demo\""
    );
    assert_eq!(provider_messages[5]["role"], "user");
    assert_eq!(provider_messages[5]["content"], "what happened?");
}

#[test]
fn tool_result_images_are_preserved_for_provider_conversion() {
    let messages = vec![
        AgentMessage::Assistant(AssistantMessage {
            content: vec![AssistantContent::ToolCall {
                id: "call_1".to_string(),
                name: "read".to_string(),
                arguments: serde_json::json!({"path": "image.png"}),
            }],
            provider: "openai".to_string(),
            model: "gpt".to_string(),
            usage: Usage::default(),
            stop_reason: StopReason::ToolUse,
            error_message: None,
            timestamp: 0,
        }),
        AgentMessage::ToolResult(ToolResultMessage {
            tool_call_id: "call_1".to_string(),
            tool_name: "read".to_string(),
            content: vec![ContentBlock::Image {
                data: "iVBORw0KGgo=".to_string(),
                mime_type: "image/png".to_string(),
            }],
            details: None,
            is_error: false,
            timestamp: 0,
        }),
    ];

    let provider_messages = messages_to_provider(&messages);
    assert_eq!(provider_messages.len(), 2);
    assert_eq!(provider_messages[1]["role"], "tool");
    let content = provider_messages[1]["content"].as_array().unwrap();
    assert_eq!(content[0]["type"], "image");
    assert_eq!(content[0]["source"]["media_type"], "image/png");
}
