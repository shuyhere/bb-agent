use super::*;
use crate::agent_session_runtime::algorithms::{
    collect_entries_for_branch_summary, estimate_context_tokens, extract_text,
    get_latest_compaction_entry,
};
use serde_json::Value;

impl AgentSessionRuntime {
    pub fn navigate_tree(
        &mut self,
        target_id: &str,
        options: NavigateTreeOptions,
        summary_text: Option<String>,
        summary_details: Option<Value>,
        source: RuntimeEntrySource,
    ) -> Result<NavigateTreeOutcome, AgentSessionRuntimeError> {
        let old_leaf_id = self.session_tree.leaf_id().map(ToOwned::to_owned);

        if old_leaf_id.as_deref() == Some(target_id) {
            return Ok(NavigateTreeOutcome {
                editor_text: None,
                cancelled: false,
                aborted: false,
                summary_entry: None,
            });
        }

        if options.summarize && self.model.is_none() {
            return Err(AgentSessionRuntimeError::NoModelForSummarization);
        }

        let target_entry = self
            .session_tree
            .get_entry(target_id)
            .cloned()
            .ok_or_else(|| AgentSessionRuntimeError::EntryNotFound(target_id.to_string()))?;

        let collected = collect_entries_for_branch_summary(
            &self.session_tree,
            old_leaf_id.as_deref(),
            target_id,
        );
        let _preparation = TreePreparation {
            target_id: target_id.to_string(),
            old_leaf_id: old_leaf_id.clone(),
            common_ancestor_id: collected.common_ancestor_id.clone(),
            entries_to_summarize: collected
                .entries
                .iter()
                .map(|entry| entry.id.clone())
                .collect(),
            user_wants_summary: options.summarize,
            custom_instructions: options.custom_instructions.clone(),
            replace_instructions: options.replace_instructions,
            label: options.label.clone(),
        };

        self.tree_state.branch_summary_in_progress = options.summarize;
        self.tree_state.abort_requested = false;

        let (new_leaf_id, editor_text) = match &target_entry.kind {
            SessionTreeEntryKind::Message(RuntimeMessage::User { content, .. }) => {
                (target_entry.parent_id.clone(), Some(extract_text(content)))
            }
            SessionTreeEntryKind::CustomMessage { content } => {
                (target_entry.parent_id.clone(), Some(extract_text(content)))
            }
            _ => (Some(target_entry.id.clone()), None),
        };

        let summary_entry = if let Some(summary) = summary_text {
            let summary_id = self.session_tree.branch_with_summary(
                new_leaf_id.clone(),
                summary,
                summary_details,
                source,
            );
            if let Some(label) = options.label {
                self.session_tree
                    .append_label_change(summary_id.clone(), label);
            }
            self.session_tree.get_entry(&summary_id).cloned()
        } else {
            match new_leaf_id.clone() {
                None => self.session_tree.reset_leaf(),
                Some(id) => self.session_tree.branch(id),
            }
            if let Some(label) = options.label {
                self.session_tree
                    .append_label_change(target_id.to_string(), label);
            }
            None
        };

        self.messages = self.session_tree.build_session_context_messages();
        self.tree_state.branch_summary_in_progress = false;
        self.tree_state.abort_requested = false;
        self.emit(RuntimeEvent::SessionTree {
            new_leaf_id: self.session_tree.leaf_id().map(ToOwned::to_owned),
            old_leaf_id,
            summary_entry_id: summary_entry.as_ref().map(|entry| entry.id.clone()),
            summary_source: summary_entry.as_ref().map(|_| source),
        });

        Ok(NavigateTreeOutcome {
            editor_text,
            cancelled: false,
            aborted: false,
            summary_entry,
        })
    }

    pub fn abort_branch_summary(&mut self) {
        self.tree_state.abort_requested = true;
    }

    pub fn get_user_messages_for_forking(&self) -> Vec<(String, String)> {
        self.session_tree
            .entries()
            .iter()
            .filter_map(|entry| match &entry.kind {
                SessionTreeEntryKind::Message(RuntimeMessage::User { content, .. }) => {
                    Some((entry.id.clone(), extract_text(content)))
                }
                _ => None,
            })
            .filter(|(_, text)| !text.is_empty())
            .collect()
    }

    pub fn get_context_usage(&self) -> Option<ContextUsage> {
        let model = self.model.as_ref()?;
        if model.context_window == 0 {
            return None;
        }

        let branch = self.session_tree.get_branch();
        let latest_compaction = get_latest_compaction_entry(&branch);
        if let Some(compaction_entry) = latest_compaction {
            let branch_ids: Vec<&str> = branch.iter().map(|entry| entry.id.as_str()).collect();
            let compaction_index = branch_ids
                .iter()
                .rposition(|id| *id == compaction_entry.id)?;
            let mut has_post_compaction_usage = false;
            for entry in branch.iter().skip(compaction_index + 1).rev() {
                if let SessionTreeEntryKind::Message(RuntimeMessage::Assistant(message)) =
                    &entry.kind
                    && message.stop_reason != AssistantStopReason::Aborted
                    && message.stop_reason != AssistantStopReason::Error
                    && message.usage.total_context_tokens() > 0
                {
                    has_post_compaction_usage = true;
                    break;
                }
            }
            if !has_post_compaction_usage {
                return Some(ContextUsage {
                    tokens: None,
                    context_window: model.context_window,
                    percent: None,
                });
            }
        }

        let estimate = estimate_context_tokens(&self.messages);
        let percent =
            ((estimate.tokens as f64 / model.context_window as f64) * 100.0).round() as u8;
        Some(ContextUsage {
            tokens: Some(estimate.tokens),
            context_window: model.context_window,
            percent: Some(percent),
        })
    }
}
