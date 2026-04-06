use bb_core::types::{
    AgentMessage, CompactionSummaryMessage, ModelInfo, SessionContext, SessionEntry, ThinkingLevel,
};

use super::formatting::{append_message, update_settings};

pub(super) fn build_context_from_entries(entries: &[SessionEntry]) -> SessionContext {
    let mut messages = Vec::new();
    let mut model: Option<ModelInfo> = None;
    let mut thinking_level = ThinkingLevel::Off;

    let compaction_idx = entries
        .iter()
        .enumerate()
        .rev()
        .find(|(_, entry)| matches!(entry, SessionEntry::Compaction { .. }))
        .map(|(idx, _)| idx);

    if let Some(comp_idx) = compaction_idx {
        let comp = &entries[comp_idx];

        if let SessionEntry::Compaction {
            summary,
            tokens_before,
            first_kept_entry_id,
            ..
        } = comp
        {
            messages.push(AgentMessage::CompactionSummary(CompactionSummaryMessage {
                summary: summary.clone(),
                tokens_before: *tokens_before,
                timestamp: comp.base().timestamp.timestamp_millis(),
            }));

            append_kept_messages_before_compaction(
                &mut messages,
                &entries[..comp_idx],
                first_kept_entry_id.as_str(),
                &mut model,
                &mut thinking_level,
            );
            append_messages_after_compaction(
                &mut messages,
                &entries[comp_idx + 1..],
                &mut model,
                &mut thinking_level,
            );
        }
    } else {
        append_messages_after_compaction(&mut messages, entries, &mut model, &mut thinking_level);
    }

    SessionContext {
        messages,
        thinking_level,
        model,
    }
}

fn append_kept_messages_before_compaction(
    messages: &mut Vec<AgentMessage>,
    entries: &[SessionEntry],
    first_kept_entry_id: &str,
    model: &mut Option<ModelInfo>,
    thinking_level: &mut ThinkingLevel,
) {
    let mut found = false;
    for entry in entries {
        if entry.base().id.as_str() == first_kept_entry_id {
            found = true;
        }
        if found {
            append_message(messages, entry);
        }
        update_settings(entry, model, thinking_level);
    }
}

fn append_messages_after_compaction(
    messages: &mut Vec<AgentMessage>,
    entries: &[SessionEntry],
    model: &mut Option<ModelInfo>,
    thinking_level: &mut ThinkingLevel,
) {
    for entry in entries {
        append_message(messages, entry);
        update_settings(entry, model, thinking_level);
    }
}
