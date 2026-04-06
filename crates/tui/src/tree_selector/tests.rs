use super::*;
use bb_session::{store::EntryRow, tree::TreeNode};

fn make_tree() -> Vec<TreeNode> {
    vec![TreeNode {
        entry_id: "root-1".to_string(),
        parent_id: None,
        entry_type: "message".to_string(),
        timestamp: "2025-01-01T00:00:00Z".to_string(),
        children: vec![
            TreeNode {
                entry_id: "child-1a".to_string(),
                parent_id: Some("root-1".to_string()),
                entry_type: "message".to_string(),
                timestamp: "2025-01-01T00:01:00Z".to_string(),
                children: vec![TreeNode {
                    entry_id: "grandchild-1".to_string(),
                    parent_id: Some("child-1a".to_string()),
                    entry_type: "compaction".to_string(),
                    timestamp: "2025-01-01T00:02:00Z".to_string(),
                    children: vec![TreeNode {
                        entry_id: "leaf-active".to_string(),
                        parent_id: Some("grandchild-1".to_string()),
                        entry_type: "message".to_string(),
                        timestamp: "2025-01-01T00:03:00Z".to_string(),
                        children: vec![],
                    }],
                }],
            },
            TreeNode {
                entry_id: "child-1b".to_string(),
                parent_id: Some("root-1".to_string()),
                entry_type: "message".to_string(),
                timestamp: "2025-01-01T00:04:00Z".to_string(),
                children: vec![TreeNode {
                    entry_id: "child-1b-resp".to_string(),
                    parent_id: Some("child-1b".to_string()),
                    entry_type: "message".to_string(),
                    timestamp: "2025-01-01T00:05:00Z".to_string(),
                    children: vec![],
                }],
            },
        ],
    }]
}

#[test]
fn test_flatten_produces_correct_count() {
    let tree = make_tree();
    let flat = flatten(&tree, 0, Some("leaf-active"), &[], &[]);
    // root + child-1a + grandchild-1 + leaf-active + child-1b + child-1b-resp = 6
    assert_eq!(flat.len(), 6);
}

#[test]
fn test_flatten_depth() {
    let tree = make_tree();
    let flat = flatten(&tree, 0, None, &[], &[]);
    assert_eq!(flat[0].depth, 0); // root-1
    assert_eq!(flat[1].depth, 1); // child-1a
    assert_eq!(flat[2].depth, 2); // grandchild-1
    assert_eq!(flat[3].depth, 3); // leaf-active
    assert_eq!(flat[4].depth, 1); // child-1b
    assert_eq!(flat[5].depth, 2); // child-1b-resp
}

#[test]
fn test_active_leaf_marked() {
    let tree = make_tree();
    let flat = flatten(&tree, 0, Some("leaf-active"), &[], &[]);
    let active_count = flat.iter().filter(|n| n.is_active).count();
    assert_eq!(active_count, 1);
    assert!(flat[3].is_active);
}

#[test]
fn test_selector_creation() {
    let tree = make_tree();
    let selector = TreeSelector::new(tree, Some("leaf-active"), 20);
    assert_eq!(selector.all_nodes.len(), 6);
    assert_eq!(selector.visible.len(), 6);
    // Should pre-select the active leaf
    assert_eq!(selector.selected, 3);
}

#[test]
fn test_selector_navigation() {
    let tree = make_tree();
    let mut selector = TreeSelector::new(tree, None, 20);
    assert_eq!(selector.selected, 0);

    selector.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert_eq!(selector.selected, 1);

    selector.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert_eq!(selector.selected, 2);

    selector.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    assert_eq!(selector.selected, 1);

    // Home
    selector.handle_key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE));
    assert_eq!(selector.selected, 5);

    selector.handle_key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE));
    assert_eq!(selector.selected, 0);
}

#[test]
fn test_selector_enter_returns_selected() {
    let tree = make_tree();
    let mut selector = TreeSelector::new(tree, None, 20);
    selector.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    match selector.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)) {
        TreeAction::Selected(id) => assert_eq!(id, "child-1a"),
        _ => panic!("expected Selected"),
    }
}

#[test]
fn test_selector_escape_cancels() {
    let tree = make_tree();
    let mut selector = TreeSelector::new(tree, None, 20);
    assert!(matches!(
        selector.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        TreeAction::Cancelled
    ));
}

#[test]
fn test_render_produces_lines() {
    let tree = make_tree();
    let selector = TreeSelector::new(tree, Some("leaf-active"), 20);
    let lines = selector.render(80);
    assert!(!lines.is_empty());
    // Should contain the active marker somewhere
    let joined = lines.join("\n");
    assert!(joined.contains("active"));
}

#[test]
fn test_render_with_small_width() {
    let tree = make_tree();
    let selector = TreeSelector::new(tree, None, 20);
    let lines = selector.render(30);
    assert!(!lines.is_empty());
}

#[test]
fn test_filter_toggle() {
    let tree = make_tree();
    let mut selector = TreeSelector::new(tree, None, 20);
    assert_eq!(selector.filter, TreeFilter::All);
    assert_eq!(selector.visible.len(), 6);

    // Toggle to UserOnly — since our basic preview doesn't start with "user:",
    // all items with entry_type "message" will match the basic "message: ..." preview.
    // Only the compaction node should be filtered out.
    selector.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
    assert_eq!(selector.filter, TreeFilter::UserOnly);
    // Without enriched previews, none start with "user:" so 0 visible
    assert_eq!(selector.visible.len(), 0);

    // Toggle back
    selector.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
    assert_eq!(selector.filter, TreeFilter::All);
    assert_eq!(selector.visible.len(), 6);
}

#[test]
fn test_scroll_with_small_visible() {
    let tree = make_tree();
    let mut selector = TreeSelector::new(tree, None, 3);
    // Navigate to end
    for _ in 0..5 {
        selector.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    }
    assert_eq!(selector.selected, 5);
    assert!(selector.scroll_offset > 0);

    let lines = selector.render(80);
    let joined = lines.join("\n");
    assert!(joined.contains("more above"));
}

#[test]
fn test_is_last_child() {
    let tree = make_tree();
    let flat = flatten(&tree, 0, None, &[], &[]);
    // root-1 is last (only) root
    assert!(flat[0].is_last_child);
    // child-1a is NOT last child (child-1b comes after)
    assert!(!flat[1].is_last_child);
    // child-1b IS last child
    assert!(flat[4].is_last_child);
}

#[test]
fn test_extract_preview_from_payload_user() {
    let payload = r#"{"id":"x","timestamp":"2025-01-01T00:00:00Z","message":{"content":[{"text":"Hello, can you help me with something?"}],"timestamp":1234}}"#;
    let preview = extract_preview_from_payload("message", payload);
    assert!(preview.starts_with("user:"));
    assert!(preview.contains("Hello"));
}

#[test]
fn test_extract_preview_from_payload_assistant() {
    let payload = r#"{"id":"x","timestamp":"2025-01-01T00:00:00Z","message":{"role":"assistant","content":[{"type":"text","text":"Sure, I can help you with that!"}],"provider":"anthropic","model":"claude","usage":{"input":0,"output":0},"stop_reason":"stop","timestamp":1234}}"#;
    let preview = extract_preview_from_payload("message", payload);
    assert!(preview.starts_with("assistant:"));
}

#[test]
fn test_extract_preview_from_payload_assistant_tool_call() {
    let payload = r#"{"type":"message","id":"x","parent_id":"p","timestamp":"2025-01-01T00:00:00Z","message":{"role":"assistant","content":[{"type":"toolCall","id":"t1","name":"bash","arguments":{"command":"echo hello world"}}],"provider":"anthropic","model":"claude","usage":{"input":0,"output":0},"stop_reason":"toolUse","timestamp":1234}}"#;
    let preview = extract_preview_from_payload("message", payload);
    assert_eq!(preview, "Bash(echo hello world)");
}

#[test]
fn test_extract_preview_from_payload_thinking() {
    let payload = r#"{"type":"message","id":"x","parent_id":"p","timestamp":"2025-01-01T00:00:00Z","message":{"role":"assistant","content":[{"type":"thinking","thinking":"considering several options for the patch"}],"provider":"anthropic","model":"claude","usage":{"input":0,"output":0},"stop_reason":"stop","timestamp":1234}}"#;
    let preview = extract_preview_from_payload("message", payload);
    assert!(preview.starts_with("think: \"considering several options"));
}

#[test]
fn test_extract_preview_from_payload_compaction() {
    let payload = r#"{"id":"x","timestamp":"2025-01-01T00:00:00Z","summary":"...","first_kept_entry_id":"y","tokens_before":12000}"#;
    let preview = extract_preview_from_payload("compaction", payload);
    assert_eq!(preview, "[compaction: 12000 tokens]");
}

#[test]
fn test_extract_preview_from_payload_tool_result() {
    let payload = r#"{"type":"message","id":"x","parent_id":"p","timestamp":"2025-01-01T00:00:00Z","message":{"role":"toolResult","tool_call_id":"t1","tool_name":"bash","content":[{"type":"text","text":"lots of output here"}],"is_error":false,"timestamp":1234}}"#;
    let preview = extract_preview_from_payload("message", payload);
    assert_eq!(preview, "[tool result: bash]");
}

#[test]
fn test_extract_preview_from_payload_model_change() {
    let payload = r#"{"id":"x","timestamp":"2025-01-01T00:00:00Z","provider":"anthropic","model_id":"claude-sonnet-4-20250514"}"#;
    let preview = extract_preview_from_payload("model_change", payload);
    assert_eq!(preview, "[model: claude-sonnet-4-20250514]");
}

#[test]
fn test_truncate_str() {
    assert_eq!(truncate_str("hello", 10), "hello");
    assert_eq!(truncate_str("hello world test", 5), "hello...");
    assert_eq!(truncate_str("line1\nline2", 20), "line1");
}

#[test]
fn build_tree_selector_only_increases_depth_on_true_forks() {
    let tree = vec![TreeNode {
        entry_id: "u1".to_string(),
        parent_id: None,
        entry_type: "message".to_string(),
        timestamp: "2025-01-01T00:00:00Z".to_string(),
        children: vec![TreeNode {
            entry_id: "a1".to_string(),
            parent_id: Some("u1".to_string()),
            entry_type: "message".to_string(),
            timestamp: "2025-01-01T00:01:00Z".to_string(),
            children: vec![TreeNode {
                entry_id: "u2".to_string(),
                parent_id: Some("a1".to_string()),
                entry_type: "message".to_string(),
                timestamp: "2025-01-01T00:02:00Z".to_string(),
                children: vec![
                    TreeNode {
                        entry_id: "a2".to_string(),
                        parent_id: Some("u2".to_string()),
                        entry_type: "message".to_string(),
                        timestamp: "2025-01-01T00:03:00Z".to_string(),
                        children: vec![],
                    },
                    TreeNode {
                        entry_id: "a3".to_string(),
                        parent_id: Some("u2".to_string()),
                        entry_type: "message".to_string(),
                        timestamp: "2025-01-01T00:04:00Z".to_string(),
                        children: vec![],
                    },
                ],
            }],
        }],
    }];
    let entries = vec![
            EntryRow { session_id: "s".into(), seq: 1, entry_id: "u1".into(), parent_id: None, entry_type: "message".into(), timestamp: "2025-01-01T00:00:00Z".into(), payload: r#"{"type":"message","id":"u1","parent_id":null,"timestamp":"2025-01-01T00:00:00Z","message":{"role":"user","content":[{"type":"text","text":"u1"}],"timestamp":1}}"#.into() },
            EntryRow { session_id: "s".into(), seq: 2, entry_id: "a1".into(), parent_id: Some("u1".into()), entry_type: "message".into(), timestamp: "2025-01-01T00:01:00Z".into(), payload: r#"{"type":"message","id":"a1","parent_id":"u1","timestamp":"2025-01-01T00:01:00Z","message":{"role":"assistant","content":[{"type":"text","text":"a1"}],"provider":"anthropic","model":"claude","usage":{"input":0,"output":0},"stop_reason":"stop","timestamp":2}}"#.into() },
            EntryRow { session_id: "s".into(), seq: 3, entry_id: "u2".into(), parent_id: Some("a1".into()), entry_type: "message".into(), timestamp: "2025-01-01T00:02:00Z".into(), payload: r#"{"type":"message","id":"u2","parent_id":"a1","timestamp":"2025-01-01T00:02:00Z","message":{"role":"user","content":[{"type":"text","text":"u2"}],"timestamp":3}}"#.into() },
            EntryRow { session_id: "s".into(), seq: 4, entry_id: "a2".into(), parent_id: Some("u2".into()), entry_type: "message".into(), timestamp: "2025-01-01T00:03:00Z".into(), payload: r#"{"type":"message","id":"a2","parent_id":"u2","timestamp":"2025-01-01T00:03:00Z","message":{"role":"assistant","content":[{"type":"text","text":"a2"}],"provider":"anthropic","model":"claude","usage":{"input":0,"output":0},"stop_reason":"stop","timestamp":4}}"#.into() },
            EntryRow { session_id: "s".into(), seq: 5, entry_id: "a3".into(), parent_id: Some("u2".into()), entry_type: "message".into(), timestamp: "2025-01-01T00:04:00Z".into(), payload: r#"{"type":"message","id":"a3","parent_id":"u2","timestamp":"2025-01-01T00:04:00Z","message":{"role":"assistant","content":[{"type":"text","text":"a3"}],"provider":"anthropic","model":"claude","usage":{"input":0,"output":0},"stop_reason":"stop","timestamp":5}}"#.into() },
        ];

    let selector = build_tree_selector(tree, &entries, Some("a2"), 20);
    let u1 = selector
        .all_nodes
        .iter()
        .find(|n| n.entry_id == "u1")
        .unwrap();
    let a1 = selector
        .all_nodes
        .iter()
        .find(|n| n.entry_id == "a1")
        .unwrap();
    let u2 = selector
        .all_nodes
        .iter()
        .find(|n| n.entry_id == "u2")
        .unwrap();
    let a2 = selector
        .all_nodes
        .iter()
        .find(|n| n.entry_id == "a2")
        .unwrap();
    let a3 = selector
        .all_nodes
        .iter()
        .find(|n| n.entry_id == "a3")
        .unwrap();

    assert_eq!(u1.depth, 0);
    assert_eq!(a1.depth, 0);
    assert_eq!(u2.depth, 0);
    assert_eq!(a2.depth, 1);
    assert_eq!(a3.depth, 1);
}

#[test]
fn test_empty_tree() {
    let selector = TreeSelector::new(vec![], None, 20);
    assert_eq!(selector.all_nodes.len(), 0);
    assert_eq!(selector.visible.len(), 0);
    let lines = selector.render(80);
    assert!(!lines.is_empty()); // header + empty message
}

#[test]
fn test_page_navigation() {
    let tree = make_tree();
    let mut selector = TreeSelector::new(tree, None, 3);
    selector.handle_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE));
    assert_eq!(selector.selected, 3);
    selector.handle_key(KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE));
    assert_eq!(selector.selected, 0);
}
