use super::*;
use bb_core::types::*;
use chrono::Utc;

fn make_user_entry(parent: Option<&str>) -> SessionEntry {
    SessionEntry::Message {
        base: EntryBase {
            id: EntryId::generate(),
            parent_id: parent.map(|s| EntryId(s.to_string())),
            timestamp: Utc::now(),
        },
        message: AgentMessage::User(UserMessage {
            content: vec![ContentBlock::Text {
                text: "hello".to_string(),
            }],
            timestamp: Utc::now().timestamp_millis(),
        }),
    }
}

#[test]
fn test_create_and_append() {
    let conn = open_memory().unwrap();
    let sid = create_session(&conn, "/tmp/test").unwrap();

    let entry = make_user_entry(None);
    let seq = append_entry(&conn, &sid, &entry).unwrap();
    assert_eq!(seq, 1);

    let entries = get_entries(&conn, &sid).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].entry_type, "message");

    let session = get_session(&conn, &sid).unwrap().unwrap();
    assert_eq!(session.entry_count, 1);
    assert!(session.leaf_id.is_some());
}

#[test]
fn test_branching() {
    let conn = open_memory().unwrap();
    let sid = create_session(&conn, "/tmp/test").unwrap();

    let e1 = make_user_entry(None);
    let e1_id = e1.base().id.clone();
    append_entry(&conn, &sid, &e1).unwrap();

    let e2 = make_user_entry(Some(e1_id.as_str()));
    append_entry(&conn, &sid, &e2).unwrap();

    set_leaf(&conn, &sid, Some(e1_id.as_str())).unwrap();
    let session = get_session(&conn, &sid).unwrap().unwrap();
    assert_eq!(session.leaf_id.as_deref(), Some(e1_id.as_str()));

    let e3 = make_user_entry(Some(e1_id.as_str()));
    append_entry(&conn, &sid, &e3).unwrap();

    let children = get_children(&conn, &sid, e1_id.as_str()).unwrap();
    assert_eq!(children.len(), 2);
}

#[test]
fn test_fork_session_from_entry_creates_new_session() {
    let conn = open_memory().unwrap();
    let sid = create_session(&conn, "/tmp/test").unwrap();

    let root = make_user_entry(None);
    let root_id = root.base().id.clone();
    append_entry(&conn, &sid, &root).unwrap();

    let middle = make_user_entry(Some(root_id.as_str()));
    let middle_id = middle.base().id.clone();
    append_entry(&conn, &sid, &middle).unwrap();

    let leaf = make_user_entry(Some(middle_id.as_str()));
    let leaf_id = leaf.base().id.clone();
    append_entry(&conn, &sid, &leaf).unwrap();

    let forked = fork_session_from_entry(&conn, &sid, leaf_id.as_str(), "/tmp/test").unwrap();
    assert_ne!(forked.session_id, sid);
    assert_eq!(forked.branch_leaf_id.as_deref(), Some(middle_id.as_str()));

    let forked_session = get_session(&conn, &forked.session_id).unwrap().unwrap();
    assert_eq!(
        forked_session.parent_session_id.as_deref(),
        Some(sid.as_str())
    );
    assert_eq!(forked_session.leaf_id.as_deref(), Some(middle_id.as_str()));

    let entries = get_entries(&conn, &forked.session_id).unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].entry_id, root_id.as_str());
    assert_eq!(entries[1].entry_id, middle_id.as_str());
}
