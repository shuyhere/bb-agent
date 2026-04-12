use super::{build_fullscreen_transcript, truncate_preview_text};
use bb_core::types::{
    AgentMessage, AssistantContent, AssistantMessage, ContentBlock, Cost, EntryBase, EntryId,
    SessionEntry, StopReason, ToolResultMessage, Usage, UserMessage,
};
use bb_session::store;
use chrono::Utc;

#[test]
fn truncate_preview_text_handles_utf8_safely() {
    let text = "你好🙂こんにちはمرحباabcdef";
    let truncated = truncate_preview_text(text, 5);
    assert_eq!(truncated, "你好🙂こん…");
}

#[test]
fn rebuild_transcript_preserves_user_image_attachment_markers() {
    let conn = store::open_memory().expect("memory db");
    let session_id = store::create_session(&conn, "/tmp").expect("session");

    let user_entry = SessionEntry::Message {
        base: EntryBase {
            id: EntryId::generate(),
            parent_id: None,
            timestamp: Utc::now(),
        },
        message: AgentMessage::User(UserMessage {
            content: vec![
                ContentBlock::Image {
                    data: "abcd".to_string(),
                    mime_type: "image/png".to_string(),
                },
                ContentBlock::Text {
                    text: "check this".to_string(),
                },
            ],
            timestamp: Utc::now().timestamp_millis(),
        }),
    };
    store::append_entry(&conn, &session_id, &user_entry).expect("append user");

    let (transcript, _) = build_fullscreen_transcript(&conn, &session_id).expect("transcript");
    let root = transcript.root_blocks()[0];
    let block = transcript.block(root).expect("user block");
    assert!(block.content.contains("[image/png attachment]"));
    assert!(block.content.contains("check this"));
}

#[test]
fn rebuild_transcript_renders_compaction_summary_as_visible_block() {
    let conn = store::open_memory().expect("memory db");
    let session_id = store::create_session(&conn, "/tmp").expect("session");

    let compaction_entry = SessionEntry::Compaction {
        base: EntryBase {
            id: EntryId::generate(),
            parent_id: None,
            timestamp: Utc::now(),
        },
        summary: "## Goal\nKeep working\n\n## Next Steps\n1. Continue".to_string(),
        first_kept_entry_id: EntryId::generate(),
        tokens_before: 12345,
        details: None,
        from_plugin: false,
    };
    store::append_entry(&conn, &session_id, &compaction_entry).expect("append compaction");

    let (transcript, _) = build_fullscreen_transcript(&conn, &session_id).expect("transcript");
    let root = transcript.root_blocks()[0];
    let block = transcript.block(root).expect("compaction block");
    assert_eq!(block.kind, bb_tui::fullscreen::BlockKind::SystemNote);
    assert_eq!(block.title, "compaction");
    assert!(block.expandable);
    assert!(block.collapsed);
    assert!(
        block
            .content
            .contains("[compaction: 12345 tokens summarized]")
    );
    assert!(block.content.contains("## Goal"));
    assert!(block.content.contains("## Next Steps"));
}

#[test]
fn rebuild_transcript_uses_shared_collapsed_tool_formatting() {
    let conn = store::open_memory().expect("memory db");
    let session_id = store::create_session(&conn, "/tmp").expect("session");

    let user_id = EntryId::generate();
    let assistant_id = EntryId::generate();

    let user_entry = SessionEntry::Message {
        base: EntryBase {
            id: user_id.clone(),
            parent_id: None,
            timestamp: Utc::now(),
        },
        message: AgentMessage::User(UserMessage {
            content: vec![ContentBlock::Text {
                text: "run something".to_string(),
            }],
            timestamp: Utc::now().timestamp_millis(),
        }),
    };
    store::append_entry(&conn, &session_id, &user_entry).expect("append user");

    let assistant_entry = SessionEntry::Message {
        base: EntryBase {
            id: assistant_id.clone(),
            parent_id: Some(user_id),
            timestamp: Utc::now(),
        },
        message: AgentMessage::Assistant(AssistantMessage {
            content: vec![AssistantContent::ToolCall {
                id: "tool-1".to_string(),
                name: "bash".to_string(),
                arguments: serde_json::json!({
                    "command": "echo one\necho two",
                    "timeout": 5
                }),
            }],
            provider: "anthropic".to_string(),
            model: "claude-sonnet-4-6".to_string(),
            usage: Usage {
                input: 0,
                output: 0,
                cache_read: 0,
                cache_write: 0,
                total_tokens: 0,
                cost: Cost::default(),
            },
            stop_reason: StopReason::ToolUse,
            error_message: None,
            timestamp: Utc::now().timestamp_millis(),
        }),
    };
    store::append_entry(&conn, &session_id, &assistant_entry).expect("append assistant");

    let tool_result_entry = SessionEntry::Message {
        base: EntryBase {
            id: EntryId::generate(),
            parent_id: Some(assistant_id),
            timestamp: Utc::now(),
        },
        message: AgentMessage::ToolResult(ToolResultMessage {
            tool_call_id: "tool-1".to_string(),
            tool_name: "bash".to_string(),
            content: vec![ContentBlock::Text {
                text: "line 1\nline 2\nline 3\nline 4".to_string(),
            }],
            details: Some(serde_json::json!({"exitCode": 0})),
            is_error: false,
            timestamp: Utc::now().timestamp_millis(),
        }),
    };
    store::append_entry(&conn, &session_id, &tool_result_entry).expect("append result");

    let (transcript, tool_states) =
        build_fullscreen_transcript(&conn, &session_id).expect("transcript");
    let assistant_root = *transcript
        .root_blocks()
        .iter()
        .find(|id| {
            transcript
                .block(**id)
                .is_some_and(|block| block.kind == bb_tui::fullscreen::BlockKind::AssistantMessage)
        })
        .expect("assistant root");
    let tool_use_id = transcript.block(assistant_root).unwrap().children[0];
    let tool_use = transcript.block(tool_use_id).unwrap();
    assert_eq!(tool_use.title, "Bash(echo one)");
    assert!(tool_use.content.is_empty());

    let tool_result_id = tool_use.children[0];
    let tool_result = transcript.block(tool_result_id).unwrap();
    let historical = tool_states.get("tool-1").expect("historical tool state");
    assert_eq!(historical.tool_use_id, tool_use_id);
    assert_eq!(historical.tool_result_id, Some(tool_result_id));
    // exit code 0 is now hidden for successful commands (no noise)
    assert!(!tool_result.content.contains("exit code: 0"));
    assert!(tool_result.content.contains("line 1"));
    assert!(
        tool_result
            .content
            .contains("click or use Ctrl+Shift+O to enter tool expand mode")
    );
    assert!(!tool_result.content.contains("\"command\""));
}
