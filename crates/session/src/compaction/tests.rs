use super::{
    SUMMARIZATION_PROMPT, calculate_context_tokens, estimate_context_tokens,
    estimate_tokens_message, estimate_tokens_text, extract_file_operations, prepare_compaction,
    serialize_conversation, should_compact,
};
use bb_core::types::{
    AgentMessage, AssistantContent, AssistantMessage, CompactionSettings, ContentBlock, EntryBase,
    SessionEntry, StopReason, ToolResultMessage, Usage, UserMessage,
};
use chrono::Utc;

use crate::store::EntryRow;

fn make_user_msg(text: &str) -> AgentMessage {
    AgentMessage::User(UserMessage {
        content: vec![ContentBlock::Text {
            text: text.to_string(),
        }],
        timestamp: 0,
    })
}

fn make_assistant_msg(
    text: &str,
    tool_calls: Vec<(&str, &str, serde_json::Value)>,
) -> AgentMessage {
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
    let messages = vec![make_assistant_msg(
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
    )];

    let (read, modified) = extract_file_operations(&messages);
    assert_eq!(read, vec!["src/main.rs"]);
    assert!(modified.contains(&"src/lib.rs".to_string()));
    assert!(modified.contains(&"src/new.rs".to_string()));
    assert!(modified.contains(&"output.txt".to_string()));
}

#[test]
fn test_extract_file_operations_deduplicates() {
    let messages = vec![make_assistant_msg(
        "",
        vec![
            ("tc1", "read", serde_json::json!({"path": "src/main.rs"})),
            ("tc2", "read", serde_json::json!({"path": "src/main.rs"})),
        ],
    )];
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
fn test_estimate_tokens_text() {
    assert_eq!(estimate_tokens_text("hello world"), 3); // ceil(11 / 4)
    assert_eq!(estimate_tokens_text("1234"), 1);
    assert_eq!(estimate_tokens_text(""), 0);
}

#[test]
fn test_calculate_context_tokens_prefers_total_tokens() {
    let usage = Usage {
        input: 10,
        output: 20,
        cache_read: 30,
        cache_write: 40,
        total_tokens: 999,
        cost: Default::default(),
        cache_metrics_source: None,
    };
    assert_eq!(calculate_context_tokens(&usage), 999);
}

#[test]
fn test_estimate_context_tokens_uses_last_assistant_usage_plus_trailing() {
    let assistant = AgentMessage::Assistant(AssistantMessage {
        content: vec![AssistantContent::Text {
            text: "done".to_string(),
        }],
        provider: "test".to_string(),
        model: "test".to_string(),
        usage: Usage {
            input: 100,
            output: 20,
            cache_read: 10,
            cache_write: 5,
            total_tokens: 0,
            cost: Default::default(),
            cache_metrics_source: None,
        },
        stop_reason: StopReason::Stop,
        error_message: None,
        timestamp: 0,
    });
    let trailing = make_user_msg("12345678"); // 2 tokens
    let estimate = estimate_context_tokens(&[assistant, trailing]);
    assert_eq!(estimate.usage_tokens, 135);
    assert_eq!(estimate.trailing_tokens, 2);
    assert_eq!(estimate.tokens, 137);
    assert_eq!(estimate.last_usage_index, Some(0));
}

#[test]
fn test_estimate_context_tokens_ignores_aborted_and_error_assistant_usage() {
    let aborted = AgentMessage::Assistant(AssistantMessage {
        content: vec![AssistantContent::Text {
            text: "aborted".to_string(),
        }],
        provider: "test".to_string(),
        model: "test".to_string(),
        usage: Usage {
            total_tokens: 500,
            ..Default::default()
        },
        stop_reason: StopReason::Aborted,
        error_message: None,
        timestamp: 0,
    });
    let user = make_user_msg("12345678"); // 2 tokens
    let estimate = estimate_context_tokens(&[aborted.clone(), user.clone()]);
    assert_eq!(estimate.last_usage_index, None);
    assert_eq!(
        estimate.tokens,
        estimate_tokens_message(&aborted) + estimate_tokens_message(&user)
    );
}

#[test]
fn test_estimate_context_tokens_ignores_usage_before_latest_compaction() {
    let before_compaction = AgentMessage::Assistant(AssistantMessage {
        content: vec![AssistantContent::Text {
            text: "before".to_string(),
        }],
        provider: "test".to_string(),
        model: "test".to_string(),
        usage: Usage {
            total_tokens: 320_000,
            ..Default::default()
        },
        stop_reason: StopReason::Stop,
        error_message: None,
        timestamp: 0,
    });
    let compaction = AgentMessage::CompactionSummary(bb_core::types::CompactionSummaryMessage {
        summary: "summary".to_string(),
        tokens_before: 320_000,
        timestamp: 1,
    });
    let after_one = make_user_msg("12345678"); // 2 tokens
    let after_two = make_user_msg("abcdefgh"); // 2 tokens

    let estimate =
        estimate_context_tokens(&[before_compaction, compaction.clone(), after_one, after_two]);
    let expected = estimate_tokens_message(&compaction)
        + estimate_tokens_message(&make_user_msg("12345678"))
        + estimate_tokens_message(&make_user_msg("abcdefgh"));

    assert_eq!(estimate.last_usage_index, None);
    assert_eq!(estimate.usage_tokens, 0);
    assert_eq!(estimate.tokens, expected);
}

#[test]
fn prepare_compaction_tokens_before_ignores_usage_before_latest_compaction() {
    let now = Utc::now();
    let before_assistant = SessionEntry::Message {
        base: EntryBase {
            id: bb_core::types::EntryId("a1".to_string()),
            parent_id: None,
            timestamp: now,
        },
        message: AgentMessage::Assistant(AssistantMessage {
            content: vec![AssistantContent::Text {
                text: "before".to_string(),
            }],
            provider: "test".to_string(),
            model: "test".to_string(),
            usage: Usage {
                total_tokens: 320_000,
                ..Default::default()
            },
            stop_reason: StopReason::Stop,
            error_message: None,
            timestamp: 0,
        }),
    };
    let compaction = SessionEntry::Compaction {
        base: EntryBase {
            id: bb_core::types::EntryId("c1".to_string()),
            parent_id: Some(bb_core::types::EntryId("a1".to_string())),
            timestamp: now,
        },
        summary: "summary".to_string(),
        first_kept_entry_id: bb_core::types::EntryId("u1".to_string()),
        tokens_before: 320_000,
        details: None,
        from_plugin: false,
    };
    let after_one = SessionEntry::Message {
        base: EntryBase {
            id: bb_core::types::EntryId("u1".to_string()),
            parent_id: Some(bb_core::types::EntryId("c1".to_string())),
            timestamp: now,
        },
        message: make_user_msg("12345678"),
    };
    let after_two = SessionEntry::Message {
        base: EntryBase {
            id: bb_core::types::EntryId("u2".to_string()),
            parent_id: Some(bb_core::types::EntryId("u1".to_string())),
            timestamp: now,
        },
        message: make_user_msg("abcdefgh"),
    };

    let entries: Vec<EntryRow> = vec![before_assistant, compaction, after_one, after_two]
        .into_iter()
        .enumerate()
        .map(|(seq, entry)| EntryRow {
            session_id: "test-session".to_string(),
            seq: seq as i64,
            entry_id: entry.base().id.to_string(),
            parent_id: entry.base().parent_id.as_ref().map(ToString::to_string),
            entry_type: match &entry {
                SessionEntry::Compaction { .. } => "compaction".to_string(),
                _ => "message".to_string(),
            },
            timestamp: entry.base().timestamp.to_rfc3339(),
            payload: serde_json::to_string(&entry).unwrap(),
        })
        .collect();

    let settings = CompactionSettings {
        keep_recent_tokens: 1,
        ..CompactionSettings::default()
    };
    let prep = prepare_compaction(&entries, &settings).expect("prep");
    let expected = estimate_tokens_message(&AgentMessage::CompactionSummary(
        bb_core::types::CompactionSummaryMessage {
            summary: "summary".to_string(),
            tokens_before: 320_000,
            timestamp: now.timestamp_millis(),
        },
    )) + estimate_tokens_message(&make_user_msg("12345678"))
        + estimate_tokens_message(&make_user_msg("abcdefgh"));

    assert_eq!(prep.tokens_before, expected);
}

#[test]
fn prepare_compaction_uses_session_entry_payloads_for_cut_detection() {
    let now = Utc::now();
    let user_entry = SessionEntry::Message {
        base: EntryBase {
            id: bb_core::types::EntryId("u1".to_string()),
            parent_id: None,
            timestamp: now,
        },
        message: make_user_msg(&"x".repeat(200)),
    };
    let branch_summary_entry = SessionEntry::Message {
        base: EntryBase {
            id: bb_core::types::EntryId("b1".to_string()),
            parent_id: Some(bb_core::types::EntryId("u1".to_string())),
            timestamp: now,
        },
        message: AgentMessage::BranchSummary(bb_core::types::BranchSummaryMessage {
            summary: "branch checkpoint".to_string(),
            from_id: "u1".to_string(),
            timestamp: 0,
        }),
    };

    let entries = vec![
        EntryRow {
            session_id: "s1".to_string(),
            seq: 1,
            entry_id: "u1".to_string(),
            parent_id: None,
            entry_type: "message".to_string(),
            timestamp: now.to_rfc3339(),
            payload: serde_json::to_string(&user_entry).unwrap(),
        },
        EntryRow {
            session_id: "s1".to_string(),
            seq: 2,
            entry_id: "b1".to_string(),
            parent_id: Some("u1".to_string()),
            entry_type: "message".to_string(),
            timestamp: now.to_rfc3339(),
            payload: serde_json::to_string(&branch_summary_entry).unwrap(),
        },
    ];

    let settings = CompactionSettings {
        enabled: true,
        reserve_tokens: 0,
        keep_recent_tokens: 1,
    };

    let preparation = prepare_compaction(&entries, &settings).expect("compaction prep");
    assert_eq!(preparation.first_kept_entry_id, "b1");
    assert_eq!(preparation.messages_to_summarize.len(), 1);
    assert_eq!(preparation.kept_messages.len(), 1);
}
