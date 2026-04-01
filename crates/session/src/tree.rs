use anyhow::Result;
use rusqlite::{params, Connection};
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
        let mut node = nodes[node_id].clone();
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store;
    use bb_core::types::*;
    use chrono::Utc;

    fn make_entry(parent: Option<&str>) -> SessionEntry {
        SessionEntry::Message {
            base: EntryBase {
                id: EntryId::generate(),
                parent_id: parent.map(|s| EntryId(s.to_string())),
                timestamp: Utc::now(),
            },
            message: AgentMessage::User(UserMessage {
                content: vec![ContentBlock::Text { text: "msg".into() }],
                timestamp: Utc::now().timestamp_millis(),
            }),
        }
    }

    #[test]
    fn test_tree_and_path() {
        let conn = store::open_memory().unwrap();
        let sid = store::create_session(&conn, "/tmp").unwrap();

        let e1 = make_entry(None);
        let e1_id = e1.base().id.as_str().to_string();
        store::append_entry(&conn, &sid, &e1).unwrap();

        let e2 = make_entry(Some(&e1_id));
        let e2_id = e2.base().id.as_str().to_string();
        store::append_entry(&conn, &sid, &e2).unwrap();

        let e3 = make_entry(Some(&e2_id));
        let e3_id = e3.base().id.as_str().to_string();
        store::append_entry(&conn, &sid, &e3).unwrap();

        // Path from e3 to root
        let path = walk_to_root(&conn, &sid, &e3_id).unwrap();
        assert_eq!(path.len(), 3);
        assert_eq!(path[0].entry_id, e1_id);
        assert_eq!(path[2].entry_id, e3_id);

        // Tree
        let tree = get_tree(&conn, &sid).unwrap();
        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].children.len(), 1);
        assert_eq!(tree[0].children[0].children.len(), 1);

        // Branch: add e4 as child of e1 (sibling of e2)
        let e4 = make_entry(Some(&e1_id));
        let e4_id = e4.base().id.as_str().to_string();
        store::append_entry(&conn, &sid, &e4).unwrap();

        let tree = get_tree(&conn, &sid).unwrap();
        assert_eq!(tree[0].children.len(), 2); // e2 and e4

        // Common ancestor of e3 and e4
        let ancestor = common_ancestor(&conn, &sid, &e3_id, &e4_id).unwrap();
        assert_eq!(ancestor, Some(e1_id));
    }
}
