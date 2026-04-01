use std::collections::HashMap;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use super::types::{
    BranchSummaryEntry, LabelChangeEntry, RuntimeMessage, SessionTreeEntry,
    SessionTreeEntryKind, StoredCompactionEntry,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionTreeState {
    entries: Vec<SessionTreeEntry>,
    leaf_id: Option<String>,
}

impl Default for SessionTreeState {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            leaf_id: None,
        }
    }
}

impl SessionTreeState {
    pub fn entries(&self) -> &[SessionTreeEntry] {
        &self.entries
    }

    pub fn leaf_id(&self) -> Option<&str> {
        self.leaf_id.as_deref()
    }

    pub fn get_entry(&self, id: &str) -> Option<&SessionTreeEntry> {
        self.entries.iter().find(|entry| entry.id == id)
    }

    pub fn get_entry_mut(&mut self, id: &str) -> Option<&mut SessionTreeEntry> {
        self.entries.iter_mut().find(|entry| entry.id == id)
    }

    pub fn append_entry(
        &mut self,
        parent_id: Option<String>,
        kind: SessionTreeEntryKind,
    ) -> String {
        let id = Uuid::new_v4().to_string();
        self.entries.push(SessionTreeEntry {
            id: id.clone(),
            parent_id,
            timestamp: Utc::now(),
            kind,
        });
        self.leaf_id = Some(id.clone());
        id
    }

    pub fn branch(&mut self, new_leaf_id: impl Into<String>) {
        self.leaf_id = Some(new_leaf_id.into());
    }

    pub fn reset_leaf(&mut self) {
        self.leaf_id = None;
    }

    pub fn branch_with_summary(
        &mut self,
        parent_id: Option<String>,
        summary: String,
        details: Option<Value>,
        from_extension: bool,
    ) -> String {
        self.append_entry(
            parent_id,
            SessionTreeEntryKind::BranchSummary(BranchSummaryEntry {
                summary,
                details,
                from_extension,
            }),
        )
    }

    pub fn append_label_change(&mut self, target_entry_id: String, label: String) -> String {
        let parent_id = self.leaf_id.clone();
        self.append_entry(
            parent_id,
            SessionTreeEntryKind::LabelChange(LabelChangeEntry {
                target_entry_id,
                label,
            }),
        )
    }

    pub fn append_compaction(
        &mut self,
        summary: String,
        first_kept_entry_id: String,
        tokens_before: usize,
        details: Option<Value>,
        from_extension: bool,
    ) -> String {
        let parent_id = self.leaf_id.clone();
        self.append_entry(
            parent_id,
            SessionTreeEntryKind::Compaction(StoredCompactionEntry {
                summary,
                first_kept_entry_id,
                tokens_before,
                details,
                from_extension,
            }),
        )
    }

    pub fn get_branch(&self) -> Vec<&SessionTreeEntry> {
        let mut by_id = HashMap::new();
        for entry in &self.entries {
            by_id.insert(entry.id.as_str(), entry);
        }

        let mut branch = Vec::new();
        let mut cursor = self.leaf_id.as_deref();
        while let Some(id) = cursor {
            if let Some(entry) = by_id.get(id) {
                branch.push(*entry);
                cursor = entry.parent_id.as_deref();
            } else {
                break;
            }
        }
        branch.reverse();
        branch
    }

    pub fn build_session_context_messages(&self) -> Vec<RuntimeMessage> {
        self.get_branch()
            .into_iter()
            .filter_map(|entry| match &entry.kind {
                SessionTreeEntryKind::Message(message) => Some(message.clone()),
                SessionTreeEntryKind::CustomMessage { content } => Some(RuntimeMessage::Custom {
                    content: content.clone(),
                    timestamp: entry.timestamp,
                }),
                _ => None,
            })
            .collect()
    }
}

