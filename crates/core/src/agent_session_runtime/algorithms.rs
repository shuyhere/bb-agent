use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use super::session_tree::SessionTreeState;
use super::types::{
    AssistantMessage, AssistantStopReason, CompactionPreparation, CompactionSettings,
    ContextEstimate, MessagePart, RuntimeMessage, SessionTreeEntry, SessionTreeEntryKind,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BranchSummaryCollection {
    pub entries: Vec<SessionTreeEntry>,
    pub common_ancestor_id: Option<String>,
}

pub fn collect_entries_for_branch_summary(
    session_tree: &SessionTreeState,
    old_leaf_id: Option<&str>,
    target_id: &str,
) -> BranchSummaryCollection {
    let by_id: HashMap<&str, &SessionTreeEntry> = session_tree
        .entries()
        .iter()
        .map(|entry| (entry.id.as_str(), entry))
        .collect();

    let target_ancestors = ancestor_chain(&by_id, Some(target_id));
    let target_ancestor_set: HashSet<&str> = target_ancestors.iter().copied().collect();

    let mut entries = Vec::new();
    let mut cursor = old_leaf_id;
    let mut common_ancestor_id = None;

    while let Some(id) = cursor {
        if target_ancestor_set.contains(id) {
            common_ancestor_id = Some(id.to_string());
            break;
        }

        let Some(entry) = by_id.get(id) else {
            break;
        };
        entries.push((*entry).clone());
        cursor = entry.parent_id.as_deref();
    }

    BranchSummaryCollection {
        entries,
        common_ancestor_id,
    }
}

pub fn prepare_compaction(
    branch: &[&SessionTreeEntry],
    settings: &CompactionSettings,
) -> Option<CompactionPreparation> {
    if branch.len() < settings.min_entries {
        return None;
    }

    let keep_recent = settings.keep_recent_entries.max(1);
    if branch.len() <= keep_recent {
        return None;
    }

    let first_kept = branch.get(branch.len().saturating_sub(keep_recent))?;
    Some(CompactionPreparation {
        path_entry_ids: branch.iter().map(|entry| entry.id.clone()).collect(),
        first_kept_entry_id: first_kept.id.clone(),
        entries_to_summarize: branch.len().saturating_sub(keep_recent),
    })
}

pub fn get_latest_compaction_entry<'a>(
    branch: &'a [&'a SessionTreeEntry],
) -> Option<&'a SessionTreeEntry> {
    branch
        .iter()
        .rev()
        .find(|entry| matches!(entry.kind, SessionTreeEntryKind::Compaction(_)))
        .copied()
}

pub fn is_context_overflow(message: &AssistantMessage, context_window: usize) -> bool {
    if message.stop_reason != AssistantStopReason::Error {
        return false;
    }

    let err = message
        .error_message
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let mentions_overflow = [
        "context overflow",
        "context length",
        "prompt is too long",
        "maximum context length",
        "too many tokens",
        "token limit",
    ]
    .iter()
    .any(|needle| err.contains(needle));

    mentions_overflow
        || (context_window > 0 && message.usage.total_context_tokens() >= context_window)
}

pub fn should_compact(
    context_tokens: usize,
    context_window: usize,
    settings: &CompactionSettings,
) -> bool {
    if context_window == 0 {
        return false;
    }

    let threshold_tokens = context_window.saturating_mul(settings.threshold_percent as usize) / 100;
    context_tokens >= threshold_tokens
        || context_window.saturating_sub(context_tokens) <= settings.reserve_tokens
}

pub fn estimate_context_tokens(messages: &[RuntimeMessage]) -> ContextEstimate {
    let mut last_usage_index = None;
    let mut tokens = 0usize;

    for (idx, message) in messages.iter().enumerate() {
        if let RuntimeMessage::Assistant(assistant) = message {
            let total = assistant.usage.total_context_tokens();
            if total > 0 {
                last_usage_index = Some(idx);
                tokens = total;
            }
        }
    }

    ContextEstimate {
        tokens,
        last_usage_index,
    }
}

pub(super) fn extract_text(content: &[MessagePart]) -> String {
    content
        .iter()
        .filter_map(|part| match part {
            MessagePart::Text { text } => Some(text.as_str()),
            MessagePart::Other { text, .. } => text.as_deref(),
            MessagePart::ToolCall { .. } => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

fn ancestor_chain<'a>(
    by_id: &HashMap<&'a str, &'a SessionTreeEntry>,
    start_id: Option<&'a str>,
) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut cursor = start_id;
    while let Some(id) = cursor {
        out.push(id);
        cursor = by_id.get(id).and_then(|entry| entry.parent_id.as_deref());
    }
    out
}
