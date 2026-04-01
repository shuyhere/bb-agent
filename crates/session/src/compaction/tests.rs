use super::{estimate_tokens_text, extract_file_operations, serialize_conversation, should_compact, SUMMARIZATION_PROMPT};
use bb_core::types::{
    AgentMessage, AssistantContent, AssistantMessage, CompactionSettings, ContentBlock,
    StopReason, ToolResultMessage, Usage, UserMessage,
};

    fn make_user_msg(text: &str) -> AgentMessage {
        AgentMessage::User(UserMessage {
            content: vec![ContentBlock::Text {
                text: text.to_string(),
            }],
            timestamp: 0,
        })
    }

    fn make_assistant_msg(text: &str, tool_calls: Vec<(&str, &str, serde_json::Value)>) -> AgentMessage {
        let mut content: Vec<AssistantContent> = Vec::new();
        if !text.is_empty() {
            content.push(AssistantContent::Text {
                text: text.to_string(),
            });
        }
        for (id, name, args) in tool_calls {
            content.push(AssistantContent::ToolCall {
                id: id.to_string(),
                name: name.to_string(),
                arguments: args,
            });
        }
        AgentMessage::Assistant(AssistantMessage {
            content,
            provider: "test".to_string(),
            model: "test".to_string(),
            usage: Usage::default(),
            stop_reason: StopReason::Stop,
            error_message: None,
            timestamp: 0,
        })
    }

    fn make_tool_result(text: &str) -> AgentMessage {
        AgentMessage::ToolResult(ToolResultMessage {
            tool_call_id: "tc1".to_string(),
            tool_name: "read".to_string(),
            content: vec![ContentBlock::Text {
                text: text.to_string(),
            }],
            details: None,
            is_error: false,
            timestamp: 0,
        })
    }

    #[test]
    fn test_serialize_conversation() {
        let messages = vec![
            make_user_msg("Hello, read a file"),
            make_assistant_msg(
                "Sure, let me read it.",
                vec![("tc1", "read", serde_json::json!({"path": "src/main.rs"}))],
            ),
            make_tool_result("fn main() {}"),
        ];

        let serialized = serialize_conversation(&messages);
        assert!(serialized.contains("[User]: Hello, read a file"));
        assert!(serialized.contains("[Assistant]: Sure, let me read it."));
        assert!(serialized.contains("[Assistant tool calls]: read(path=\"src/main.rs\")"));
        assert!(serialized.contains("[Tool result]: fn main() {}"));
    }

    #[test]
    fn test_serialize_conversation_truncates_tool_result() {
        let long_text = "x".repeat(3000);
        let messages = vec![make_tool_result(&long_text)];
        let serialized = serialize_conversation(&messages);
        assert!(serialized.contains("...(truncated)"));
        // Should contain first 2000 chars
        assert!(serialized.contains(&"x".repeat(2000)));
    }

    #[test]
    fn test_extract_file_operations() {
        let messages = vec![
            make_assistant_msg(
                "",
                vec![
                    ("tc1", "read", serde_json::json!({"path": "src/main.rs"})),
                    ("tc2", "edit", serde_json::json!({"path": "src/lib.rs"})),
                    ("tc3", "write", serde_json::json!({"path": "src/new.rs"})),
                    (
                        "tc4",
                        "bash",
                        serde_json::json!({"command": "echo hello > output.txt"}),
                    ),
                ],
            ),
        ];

        let (read, modified) = extract_file_operations(&messages);
        assert_eq!(read, vec!["src/main.rs"]);
        assert!(modified.contains(&"src/lib.rs".to_string()));
        assert!(modified.contains(&"src/new.rs".to_string()));
        assert!(modified.contains(&"output.txt".to_string()));
    }

    #[test]
    fn test_extract_file_operations_deduplicates() {
        let messages = vec![
            make_assistant_msg(
                "",
                vec![
                    ("tc1", "read", serde_json::json!({"path": "src/main.rs"})),
                    ("tc2", "read", serde_json::json!({"path": "src/main.rs"})),
                ],
            ),
        ];
        let (read, _) = extract_file_operations(&messages);
        assert_eq!(read, vec!["src/main.rs"]);
    }

    #[test]
    fn test_summarization_prompt_format() {
        assert!(SUMMARIZATION_PROMPT.contains("## Goal"));
        assert!(SUMMARIZATION_PROMPT.contains("## Constraints & Preferences"));
        assert!(SUMMARIZATION_PROMPT.contains("## Progress"));
        assert!(SUMMARIZATION_PROMPT.contains("### Done"));
        assert!(SUMMARIZATION_PROMPT.contains("### In Progress"));
        assert!(SUMMARIZATION_PROMPT.contains("## Key Decisions"));
        assert!(SUMMARIZATION_PROMPT.contains("## Next Steps"));
        assert!(SUMMARIZATION_PROMPT.contains("## Critical Context"));
    }

    #[test]
    fn test_should_compact() {
        let settings = CompactionSettings::default();
        // 128K context, 100K used — should not compact (100K < 128K - 16K = 112K)
        assert!(!should_compact(100_000, 128_000, &settings));
        // 120K used — should compact (120K > 112K)
        assert!(should_compact(120_000, 128_000, &settings));
    }

    #[test]
    fn test_should_compact_triggers() {
        let settings = CompactionSettings::default(); // reserve=16384
        assert!(should_compact(120_000, 128_000, &settings)); // over threshold
        assert!(!should_compact(100_000, 128_000, &settings)); // under threshold
    }

    #[test]
    fn test_estimate_tokens() {
        assert_eq!(estimate_tokens_text("hello world"), 2); // 11 chars / 4
        assert_eq!(estimate_tokens_text(""), 0);
    }