use super::*;
use crate::store;
use bb_core::types::*;
use chrono::Utc;

#[test]
fn test_build_context_empty() {
    let conn = store::open_memory().unwrap();
    let sid = store::create_session(&conn, "/tmp").unwrap();
    let ctx = build_context(&conn, &sid).unwrap();
    assert!(ctx.messages.is_empty());
}

#[test]
fn test_explicit_thinking_level_from_path_is_none_when_unset() {
    let conn = store::open_memory().unwrap();
    let sid = store::create_session(&conn, "/tmp").unwrap();

    let e1 = SessionEntry::Message {
        base: EntryBase {
            id: EntryId::generate(),
            parent_id: None,
            timestamp: Utc::now(),
        },
        message: AgentMessage::User(UserMessage {
            content: vec![ContentBlock::Text {
                text: "hello".into(),
            }],
            timestamp: Utc::now().timestamp_millis(),
        }),
    };
    store::append_entry(&conn, &sid, &e1).unwrap();

    assert_eq!(
        active_path_explicit_thinking_level(&conn, &sid).unwrap(),
        None
    );
}

#[test]
fn test_explicit_thinking_level_from_path_returns_latest_change() {
    let conn = store::open_memory().unwrap();
    let sid = store::create_session(&conn, "/tmp").unwrap();

    let root_id = EntryId::generate();
    let root = SessionEntry::Message {
        base: EntryBase {
            id: root_id.clone(),
            parent_id: None,
            timestamp: Utc::now(),
        },
        message: AgentMessage::User(UserMessage {
            content: vec![ContentBlock::Text {
                text: "hello".into(),
            }],
            timestamp: Utc::now().timestamp_millis(),
        }),
    };
    store::append_entry(&conn, &sid, &root).unwrap();

    let low_id = EntryId::generate();
    let low = SessionEntry::ThinkingLevelChange {
        base: EntryBase {
            id: low_id.clone(),
            parent_id: Some(root_id),
            timestamp: Utc::now(),
        },
        thinking_level: ThinkingLevel::Low,
    };
    store::append_entry(&conn, &sid, &low).unwrap();

    let high = SessionEntry::ThinkingLevelChange {
        base: EntryBase {
            id: EntryId::generate(),
            parent_id: Some(low_id),
            timestamp: Utc::now(),
        },
        thinking_level: ThinkingLevel::High,
    };
    store::append_entry(&conn, &sid, &high).unwrap();

    assert_eq!(
        active_path_explicit_thinking_level(&conn, &sid).unwrap(),
        Some(ThinkingLevel::High)
    );
}

#[test]
fn test_build_context_simple() {
    let conn = store::open_memory().unwrap();
    let sid = store::create_session(&conn, "/tmp").unwrap();

    let e1 = SessionEntry::Message {
        base: EntryBase {
            id: EntryId::generate(),
            parent_id: None,
            timestamp: Utc::now(),
        },
        message: AgentMessage::User(UserMessage {
            content: vec![ContentBlock::Text {
                text: "hello".into(),
            }],
            timestamp: Utc::now().timestamp_millis(),
        }),
    };
    store::append_entry(&conn, &sid, &e1).unwrap();

    let ctx = build_context(&conn, &sid).unwrap();
    assert_eq!(ctx.messages.len(), 1);
    assert!(matches!(ctx.messages[0], AgentMessage::User(_)));
}

#[test]
fn test_build_context_with_compaction() {
    let conn = store::open_memory().unwrap();
    let sid = store::create_session(&conn, "/tmp").unwrap();

    let e1 = SessionEntry::Message {
        base: EntryBase {
            id: EntryId("e1000001".into()),
            parent_id: None,
            timestamp: Utc::now(),
        },
        message: AgentMessage::User(UserMessage {
            content: vec![ContentBlock::Text {
                text: "old msg".into(),
            }],
            timestamp: 1000,
        }),
    };
    store::append_entry(&conn, &sid, &e1).unwrap();

    let e2 = SessionEntry::Message {
        base: EntryBase {
            id: EntryId("e2000002".into()),
            parent_id: Some(EntryId("e1000001".into())),
            timestamp: Utc::now(),
        },
        message: AgentMessage::User(UserMessage {
            content: vec![ContentBlock::Text {
                text: "kept msg".into(),
            }],
            timestamp: 2000,
        }),
    };
    store::append_entry(&conn, &sid, &e2).unwrap();

    let e3 = SessionEntry::Compaction {
        base: EntryBase {
            id: EntryId("e3000003".into()),
            parent_id: Some(EntryId("e2000002".into())),
            timestamp: Utc::now(),
        },
        summary: "Summary of old conversation".into(),
        first_kept_entry_id: EntryId("e2000002".into()),
        tokens_before: 5000,
        details: None,
        from_plugin: false,
    };
    store::append_entry(&conn, &sid, &e3).unwrap();

    let e4 = SessionEntry::Message {
        base: EntryBase {
            id: EntryId("e4000004".into()),
            parent_id: Some(EntryId("e3000003".into())),
            timestamp: Utc::now(),
        },
        message: AgentMessage::User(UserMessage {
            content: vec![ContentBlock::Text {
                text: "new msg".into(),
            }],
            timestamp: 4000,
        }),
    };
    store::append_entry(&conn, &sid, &e4).unwrap();

    let ctx = build_context(&conn, &sid).unwrap();

    assert_eq!(ctx.messages.len(), 3);
    assert!(matches!(
        ctx.messages[0],
        AgentMessage::CompactionSummary(_)
    ));
    assert!(matches!(ctx.messages[1], AgentMessage::User(_)));
    assert!(matches!(ctx.messages[2], AgentMessage::User(_)));

    if let AgentMessage::User(u) = &ctx.messages[1] {
        assert_eq!(
            u.content[0],
            ContentBlock::Text {
                text: "kept msg".into()
            }
        );
    }
}
