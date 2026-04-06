use anyhow::Result;
use bb_core::types::{AgentMessage, ContentBlock, SessionEntry};
use rusqlite::Connection;
use std::collections::{HashMap, HashSet};

use crate::store::EntryRow;

/// A tree node for display.
#[derive(Clone, Debug)]
pub struct TreeNode {
    pub entry_id: String,
    pub parent_id: Option<String>,
    pub entry_type: String,
    pub timestamp: String,
    pub children: Vec<TreeNode>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeSummaryCollection {
    pub entries: Vec<EntryRow>,
    pub common_ancestor_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeTargetResolution {
    pub new_leaf_id: Option<String>,
    pub editor_text: Option<String>,
    pub target_entry_type: String,
}

/// Walk from an entry to the root, returning the path root → entry.
pub fn walk_to_root(conn: &Connection, session_id: &str, entry_id: &str) -> Result<Vec<EntryRow>> {
    let mut path = Vec::new();
    let mut current_id = Some(entry_id.to_string());

    while let Some(id) = current_id {
        let row = crate::store::get_entry(conn, session_id, &id)?;
        match row {
            Some(r) => {
                current_id = r.parent_id.clone();
                path.push(r);
            }
            None => break,
        }
    }

    path.reverse(); // root → leaf
    Ok(path)
}

/// Get the active path from root to current leaf.
pub fn active_path(conn: &Connection, session_id: &str) -> Result<Vec<EntryRow>> {
    let session = crate::store::get_session(conn, session_id)?;
    match session {
        Some(s) => match s.leaf_id {
            Some(leaf) => walk_to_root(conn, session_id, &leaf),
            None => Ok(Vec::new()),
        },
        None => Ok(Vec::new()),
    }
}

/// Build the full tree structure for a session.
pub fn get_tree(conn: &Connection, session_id: &str) -> Result<Vec<TreeNode>> {
    let entries = crate::store::get_entries(conn, session_id)?;

    let mut nodes: HashMap<String, TreeNode> = HashMap::new();
    let mut roots = Vec::new();
    let mut child_map: HashMap<String, Vec<String>> = HashMap::new();

    // Create nodes
    for entry in &entries {
        nodes.insert(
            entry.entry_id.clone(),
            TreeNode {
                entry_id: entry.entry_id.clone(),
                parent_id: entry.parent_id.clone(),
                entry_type: entry.entry_type.clone(),
                timestamp: entry.timestamp.clone(),
                children: Vec::new(),
            },
        );

        match &entry.parent_id {
            Some(pid) => {
                child_map
                    .entry(pid.clone())
                    .or_default()
                    .push(entry.entry_id.clone());
            }
            None => roots.push(entry.entry_id.clone()),
        }
    }

    // Build tree recursively
    fn build(
        node_id: &str,
        nodes: &HashMap<String, TreeNode>,
        child_map: &HashMap<String, Vec<String>>,
    ) -> TreeNode {
        let Some(base) = nodes.get(node_id) else {
            return TreeNode {
                entry_id: node_id.to_string(),
                parent_id: None,
                entry_type: "unknown".into(),
                timestamp: String::new(),
                children: Vec::new(),
            };
        };
        let mut node = base.clone();
        if let Some(children_ids) = child_map.get(node_id) {
            node.children = children_ids
                .iter()
                .map(|cid| build(cid, nodes, child_map))
                .collect();
            // Sort children by timestamp
            node.children.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
        }
        node
    }

    let tree = roots
        .iter()
        .map(|rid| build(rid, &nodes, &child_map))
        .collect();

    Ok(tree)
}

/// Find the common ancestor of two entries.
pub fn common_ancestor(
    conn: &Connection,
    session_id: &str,
    a_id: &str,
    b_id: &str,
) -> Result<Option<String>> {
    let path_a = walk_to_root(conn, session_id, a_id)?;
    let set_a: HashSet<String> = path_a.iter().map(|e| e.entry_id.clone()).collect();

    let path_b = walk_to_root(conn, session_id, b_id)?;

    // Walk b's path from leaf to root; first match is deepest common ancestor
    for entry in path_b.iter().rev() {
        if set_a.contains(&entry.entry_id) {
            return Ok(Some(entry.entry_id.clone()));
        }
    }

    Ok(None)
}

pub fn collect_entries_for_branch_summary(
    conn: &Connection,
    session_id: &str,
    old_leaf_id: Option<&str>,
    target_id: &str,
) -> Result<TreeSummaryCollection> {
    let Some(old_leaf_id) = old_leaf_id else {
        return Ok(TreeSummaryCollection {
            entries: Vec::new(),
            common_ancestor_id: None,
        });
    };

    let common_ancestor_id = common_ancestor(conn, session_id, old_leaf_id, target_id)?;
    let mut entries = Vec::new();
    let mut current_id = Some(old_leaf_id.to_string());

    while let Some(id) = current_id {
        if common_ancestor_id.as_deref() == Some(id.as_str()) {
            break;
        }
        let Some(row) = crate::store::get_entry(conn, session_id, &id)? else {
            break;
        };
        current_id = row.parent_id.clone();
        entries.push(row);
    }

    entries.reverse();

    Ok(TreeSummaryCollection {
        entries,
        common_ancestor_id,
    })
}

pub fn resolve_tree_target(
    conn: &Connection,
    session_id: &str,
    target_id: &str,
) -> Result<TreeTargetResolution> {
    let row = crate::store::get_entry(conn, session_id, target_id)?
        .ok_or_else(|| anyhow::anyhow!("Entry not found: {target_id}"))?;
    let entry = crate::store::parse_entry(&row)?;

    let resolution = match entry {
        SessionEntry::Message {
            message: AgentMessage::User(user),
            ..
        } => TreeTargetResolution {
            new_leaf_id: row.parent_id.clone(),
            editor_text: Some(text_from_blocks(&user.content)),
            target_entry_type: "user".to_string(),
        },
        SessionEntry::CustomMessage { content, .. } => TreeTargetResolution {
            new_leaf_id: row.parent_id.clone(),
            editor_text: Some(text_from_blocks(&content)),
            target_entry_type: "custom_message".to_string(),
        },
        other => TreeTargetResolution {
            new_leaf_id: Some(target_id.to_string()),
            editor_text: None,
            target_entry_type: other.entry_type().to_string(),
        },
    };

    Ok(resolution)
}

fn text_from_blocks(blocks: &[ContentBlock]) -> String {
    blocks
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store;
    use bb_core::types::*;
    use chrono::Utc;

    fn make_user_entry(parent: Option<&str>, text: &str) -> SessionEntry {
        SessionEntry::Message {
            base: EntryBase {
                id: EntryId::generate(),
                parent_id: parent.map(|s| EntryId(s.to_string())),
                timestamp: Utc::now(),
            },
            message: AgentMessage::User(UserMessage {
                content: vec![ContentBlock::Text { text: text.into() }],
                timestamp: Utc::now().timestamp_millis(),
            }),
        }
    }

    fn make_assistant_entry(parent: Option<&str>, text: &str) -> SessionEntry {
        SessionEntry::Message {
            base: EntryBase {
                id: EntryId::generate(),
                parent_id: parent.map(|s| EntryId(s.to_string())),
                timestamp: Utc::now(),
            },
            message: AgentMessage::Assistant(AssistantMessage {
                content: vec![AssistantContent::Text { text: text.into() }],
                provider: "test".into(),
                model: "test".into(),
                usage: Usage::default(),
                stop_reason: StopReason::Stop,
                error_message: None,
                timestamp: Utc::now().timestamp_millis(),
            }),
        }
    }

    #[test]
    fn test_tree_and_path() {
        let conn = store::open_memory().unwrap();
        let sid = store::create_session(&conn, "/tmp").unwrap();

        let e1 = make_user_entry(None, "msg");
        let e1_id = e1.base().id.as_str().to_string();
        store::append_entry(&conn, &sid, &e1).unwrap();

        let e2 = make_user_entry(Some(&e1_id), "msg");
        let e2_id = e2.base().id.as_str().to_string();
        store::append_entry(&conn, &sid, &e2).unwrap();

        let e3 = make_user_entry(Some(&e2_id), "msg");
        let e3_id = e3.base().id.as_str().to_string();
        store::append_entry(&conn, &sid, &e3).unwrap();

        let path = walk_to_root(&conn, &sid, &e3_id).unwrap();
        assert_eq!(path.len(), 3);
        assert_eq!(path[0].entry_id, e1_id);
        assert_eq!(path[2].entry_id, e3_id);

        let tree = get_tree(&conn, &sid).unwrap();
        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].children.len(), 1);
        assert_eq!(tree[0].children[0].children.len(), 1);

        let e4 = make_user_entry(Some(&e1_id), "msg");
        let e4_id = e4.base().id.as_str().to_string();
        store::append_entry(&conn, &sid, &e4).unwrap();

        let tree = get_tree(&conn, &sid).unwrap();
        assert_eq!(tree[0].children.len(), 2);

        let ancestor = common_ancestor(&conn, &sid, &e3_id, &e4_id).unwrap();
        assert_eq!(ancestor, Some(e1_id));
    }

    #[test]
    fn selecting_user_message_resolves_to_parent_and_editor_text() {
        let conn = store::open_memory().unwrap();
        let sid = store::create_session(&conn, "/tmp").unwrap();

        let root = make_user_entry(None, "root");
        let root_id = root.base().id.as_str().to_string();
        store::append_entry(&conn, &sid, &root).unwrap();

        let user = make_user_entry(Some(&root_id), "continue here");
        let user_id = user.base().id.as_str().to_string();
        store::append_entry(&conn, &sid, &user).unwrap();

        let resolved = resolve_tree_target(&conn, &sid, &user_id).unwrap();
        assert_eq!(resolved.new_leaf_id, Some(root_id));
        assert_eq!(resolved.editor_text.as_deref(), Some("continue here"));
        assert_eq!(resolved.target_entry_type, "user");
    }

    #[test]
    fn selecting_assistant_message_resolves_to_selected_entry() {
        let conn = store::open_memory().unwrap();
        let sid = store::create_session(&conn, "/tmp").unwrap();

        let root = make_user_entry(None, "root");
        let root_id = root.base().id.as_str().to_string();
        store::append_entry(&conn, &sid, &root).unwrap();

        let assistant = make_assistant_entry(Some(&root_id), "done");
        let assistant_id = assistant.base().id.as_str().to_string();
        store::append_entry(&conn, &sid, &assistant).unwrap();

        let resolved = resolve_tree_target(&conn, &sid, &assistant_id).unwrap();
        assert_eq!(resolved.new_leaf_id, Some(assistant_id));
        assert!(resolved.editor_text.is_none());
        assert_eq!(resolved.target_entry_type, "message");
    }
}
