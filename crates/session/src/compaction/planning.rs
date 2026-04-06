use super::types::*;
use crate::store::EntryRow;
use bb_core::types::{AgentMessage, CompactionSettings, SessionEntry};

/// Whether compaction should trigger.
pub fn should_compact(
    context_tokens: u64,
    context_window: u64,
    settings: &CompactionSettings,
) -> bool {
    settings.enabled && context_tokens > context_window.saturating_sub(settings.reserve_tokens)
}

/// Estimate tokens for a message (rough: ~4 chars per token).
pub fn estimate_tokens_text(text: &str) -> u64 {
    (text.len() as u64) / 4
}

/// Estimate tokens for an entry row by its payload size.
pub fn estimate_tokens_row(row: &EntryRow) -> u64 {
    estimate_tokens_text(&row.payload)
}

/// Find the cut point that keeps approximately `keep_recent_tokens`.
///
/// Walks backward from the newest entry, accumulating token estimates.
/// Returns the index of the first entry to keep.
pub fn find_cut_point(
    entries: &[EntryRow],
    start: usize,
    end: usize,
    keep_recent_tokens: u64,
) -> usize {
    let mut accumulated: u64 = 0;
    let mut cut = start;

    for i in (start..end).rev() {
        let entry = &entries[i];
        if entry.entry_type != "message" {
            continue;
        }
        let tokens = estimate_tokens_row(entry);
        accumulated += tokens;

        if accumulated >= keep_recent_tokens {
            // Find valid cut point at or after this index
            cut = find_valid_cut_at_or_after(entries, i, start, end);
            break;
        }
    }

    cut
}

/// Find the nearest valid cut point at or after `idx`.
/// Valid: user message, assistant message, bash execution. Not: tool result.
fn find_valid_cut_at_or_after(entries: &[EntryRow], idx: usize, start: usize, end: usize) -> usize {
    for (i, entry) in entries.iter().enumerate().take(end).skip(idx) {
        if entry.entry_type != "message" {
            continue;
        }
        if is_valid_cut_row(entry) {
            return i;
        }
    }
    // Fallback: start of range
    start
}

/// Check if an entry row allows cutting here.
fn is_valid_cut_row(row: &EntryRow) -> bool {
    let Ok(entry) = serde_json::from_str::<SessionEntry>(&row.payload) else {
        return false;
    };
    matches!(
        entry,
        SessionEntry::Message {
            message: AgentMessage::User(_)
                | AgentMessage::Assistant(_)
                | AgentMessage::BashExecution(_)
                | AgentMessage::Custom(_)
                | AgentMessage::BranchSummary(_),
            ..
        }
    )
}

/// Prepare compaction data from the active path entries.
pub fn prepare_compaction(
    path_entries: &[EntryRow],
    settings: &CompactionSettings,
) -> Option<CompactionPreparation> {
    if path_entries.is_empty() {
        return None;
    }

    // Don't compact right after a compaction
    if path_entries.last().map(|e| e.entry_type.as_str()) == Some("compaction") {
        return None;
    }

    // Find previous compaction
    let prev_compaction_idx = path_entries
        .iter()
        .enumerate()
        .rev()
        .find(|(_, e)| e.entry_type == "compaction")
        .map(|(i, _)| i);

    let mut previous_summary = None;
    let boundary_start = if let Some(pc_idx) = prev_compaction_idx {
        previous_summary = extract_summary(&path_entries[pc_idx]);
        let first_kept = extract_first_kept_id(&path_entries[pc_idx]);
        if let Some(fk) = first_kept {
            path_entries
                .iter()
                .position(|e| e.entry_id == fk)
                .unwrap_or(pc_idx + 1)
        } else {
            pc_idx + 1
        }
    } else {
        0
    };

    let boundary_end = path_entries.len();

    // Estimate current context tokens
    let tokens_before: u64 = path_entries.iter().map(estimate_tokens_row).sum();

    // Find cut point
    let cut = find_cut_point(
        path_entries,
        boundary_start,
        boundary_end,
        settings.keep_recent_tokens,
    );

    if cut <= boundary_start {
        return None; // Nothing to summarize
    }

    let first_kept_entry = &path_entries[cut];

    let messages_to_summarize = path_entries[boundary_start..cut].to_vec();
    let kept_messages = path_entries[cut..].to_vec();

    Some(CompactionPreparation {
        first_kept_entry_id: first_kept_entry.entry_id.clone(),
        messages_to_summarize,
        kept_messages,
        tokens_before,
        previous_summary,
        is_split_turn: false, // Simplified; full split-turn logic added later
    })
}

fn extract_summary(row: &EntryRow) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(&row.payload).ok()?;
    v.get("summary")
        .and_then(|s| s.as_str())
        .map(|s| s.to_string())
}

fn extract_first_kept_id(row: &EntryRow) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(&row.payload).ok()?;
    v.get("first_kept_entry_id")
        .and_then(|s| s.as_str())
        .map(|s| s.to_string())
}
