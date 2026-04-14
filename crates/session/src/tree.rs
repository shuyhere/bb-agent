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

impl TreeNode {
    fn from_row(row: &EntryRow, children: Vec<TreeNode>) -> Self {
        Self {
            entry_id: row.entry_id.clone(),
            parent_id: row.parent_id.clone(),
            entry_type: row.entry_type.clone(),
            timestamp: row.timestamp.clone(),
            children,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeSummaryCollection {
    entries: Vec<EntryRow>,
    common_ancestor_id: Option<String>,
}

impl TreeSummaryCollection {
    pub fn entries(&self) -> &[EntryRow] {
        &self.entries
    }

    pub fn common_ancestor_id(&self) -> Option<&str> {
        self.common_ancestor_id.as_deref()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TreeTargetKind {
    User,
    CustomMessage,
    Message,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeTargetResolution {
    new_leaf_id: Option<String>,
    editor_text: Option<String>,
    target_kind: TreeTargetKind,
}

impl TreeTargetResolution {
    pub fn new_leaf_id(&self) -> Option<&str> {
        self.new_leaf_id.as_deref()
    }

    pub fn into_new_leaf_id(self) -> Option<String> {
        self.new_leaf_id
    }

    pub fn editor_text(&self) -> Option<&str> {
        self.editor_text.as_deref()
    }

    pub fn into_editor_text(self) -> Option<String> {
        self.editor_text
    }

    pub fn target_kind(&self) -> TreeTargetKind {
        self.target_kind
    }
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
    let rows_by_id: HashMap<&str, &EntryRow> = entries
        .iter()
        .map(|entry| (entry.entry_id.as_str(), entry))
        .collect();
    let mut roots = Vec::new();
    let mut child_map: HashMap<&str, Vec<&str>> = HashMap::new();

    for entry in &entries {
        match entry.parent_id.as_deref() {
            Some(parent_id) => child_map
                .entry(parent_id)
                .or_default()
                .push(entry.entry_id.as_str()),
            None => roots.push(entry.entry_id.as_str()),
        }
    }

    fn build(
        node_id: &str,
        rows_by_id: &HashMap<&str, &EntryRow>,
        child_map: &HashMap<&str, Vec<&str>>,
    ) -> TreeNode {
        let Some(row) = rows_by_id.get(node_id) else {
            return TreeNode {
                entry_id: node_id.to_string(),
                parent_id: None,
                entry_type: "unknown".into(),
                timestamp: String::new(),
                children: Vec::new(),
            };
        };

        let mut children = child_map
            .get(node_id)
            .into_iter()
            .flatten()
            .map(|child_id| build(child_id, rows_by_id, child_map))
            .collect::<Vec<_>>();
        children.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));

        TreeNode::from_row(row, children)
    }

    Ok(roots
        .into_iter()
        .map(|root_id| build(root_id, &rows_by_id, &child_map))
        .collect())
}

/// Find the common ancestor of two entries.
pub fn common_ancestor(
    conn: &Connection,
    session_id: &str,
    a_id: &str,
    b_id: &str,
) -> Result<Option<String>> {
    let path_a = walk_to_root(conn, session_id, a_id)?;
    let set_a: HashSet<&str> = path_a.iter().map(|entry| entry.entry_id.as_str()).collect();

    let path_b = walk_to_root(conn, session_id, b_id)?;

    // Walk b's path from leaf to root; the first match is the deepest common ancestor.
    for entry in path_b.iter().rev() {
        if set_a.contains(entry.entry_id.as_str()) {
            return Ok(Some(entry.entry_id.clone()));
        }
    }

    Ok(None)
}

/// Collect the abandoned branch segment between the current leaf and the target's common
/// ancestor. Returned rows stay ordered from oldest → newest so branch-summary generation can
/// serialize them directly without another reversal step.
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
            target_kind: TreeTargetKind::User,
        },
        SessionEntry::CustomMessage { content, .. } => TreeTargetResolution {
            new_leaf_id: row.parent_id.clone(),
            editor_text: Some(text_from_blocks(&content)),
            target_kind: TreeTargetKind::CustomMessage,
        },
        SessionEntry::Message { .. } => TreeTargetResolution {
            new_leaf_id: Some(target_id.to_string()),
            editor_text: None,
            target_kind: TreeTargetKind::Message,
        },
        _ => TreeTargetResolution {
            new_leaf_id: Some(target_id.to_string()),
            editor_text: None,
            target_kind: TreeTargetKind::Other,
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
        assert_eq!(resolved.new_leaf_id(), Some(root_id.as_str()));
        assert_eq!(resolved.editor_text(), Some("continue here"));
        assert_eq!(resolved.target_kind(), TreeTargetKind::User);
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
        assert_eq!(resolved.new_leaf_id(), Some(assistant_id.as_str()));
        assert!(resolved.editor_text().is_none());
        assert_eq!(resolved.target_kind(), TreeTargetKind::Message);
    }

    #[test]
    fn collecting_branch_summary_without_active_leaf_is_empty() {
        let conn = store::open_memory().unwrap();
        let sid = store::create_session(&conn, "/tmp").unwrap();
        let root = make_user_entry(None, "root");
        let root_id = root.base().id.as_str().to_string();
        store::append_entry(&conn, &sid, &root).unwrap();

        let collected = collect_entries_for_branch_summary(&conn, &sid, None, &root_id).unwrap();
        assert!(collected.is_empty());
        assert_eq!(collected.common_ancestor_id(), None);
        assert!(collected.entries().is_empty());
    }

    #[test]
    fn collecting_branch_summary_returns_ordered_abandoned_entries() {
        let conn = store::open_memory().unwrap();
        let sid = store::create_session(&conn, "/tmp").unwrap();

        let root = make_user_entry(None, "root");
        let root_id = root.base().id.as_str().to_string();
        store::append_entry(&conn, &sid, &root).unwrap();

        let branch_a = make_user_entry(Some(&root_id), "branch-a");
        let branch_a_id = branch_a.base().id.as_str().to_string();
        store::append_entry(&conn, &sid, &branch_a).unwrap();

        let branch_b = make_assistant_entry(Some(&branch_a_id), "branch-b");
        let branch_b_id = branch_b.base().id.as_str().to_string();
        store::append_entry(&conn, &sid, &branch_b).unwrap();

        let sibling = make_user_entry(Some(&root_id), "sibling");
        let sibling_id = sibling.base().id.as_str().to_string();
        store::append_entry(&conn, &sid, &sibling).unwrap();

        let collected =
            collect_entries_for_branch_summary(&conn, &sid, Some(&branch_b_id), &sibling_id)
                .unwrap();

        assert_eq!(collected.common_ancestor_id(), Some(root_id.as_str()));
        let entry_ids: Vec<&str> = collected
            .entries()
            .iter()
            .map(|row| row.entry_id.as_str())
            .collect();
        assert_eq!(entry_ids, vec![branch_a_id.as_str(), branch_b_id.as_str()]);
    }
}
