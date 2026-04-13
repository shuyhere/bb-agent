use anyhow::Result;
use bb_core::types::{SessionContext, SessionEntry, ThinkingLevel};
use rusqlite::Connection;

use crate::store::{self, EntryRow};
use crate::tree;

mod assembly;
mod formatting;
#[cfg(test)]
mod tests;

/// Build the session context (what gets sent to the LLM).
///
/// Walks root → leaf, applies compaction boundary, returns messages.
pub fn build_context(conn: &Connection, session_id: &str) -> Result<SessionContext> {
    let path = tree::active_path(conn, session_id)?;
    build_context_from_path(&path)
}

/// Return the latest explicitly recorded thinking level on the active path, if any.
///
/// This scans the full active path without applying compaction boundaries so resume
/// logic can distinguish an actual persisted `off` from "no explicit thinking level
/// was ever recorded for this session".
pub fn active_path_explicit_thinking_level(
    conn: &Connection,
    session_id: &str,
) -> Result<Option<ThinkingLevel>> {
    let path = tree::active_path(conn, session_id)?;
    explicit_thinking_level_from_path(&path)
}

/// Return the latest explicitly recorded thinking level on a path, if any.
pub fn explicit_thinking_level_from_path(path: &[EntryRow]) -> Result<Option<ThinkingLevel>> {
    for row in path.iter().rev() {
        if let SessionEntry::ThinkingLevelChange { thinking_level, .. } = store::parse_entry(row)? {
            return Ok(Some(thinking_level));
        }
    }
    Ok(None)
}

/// Build context from a pre-computed path (for testing / reuse).
pub fn build_context_from_path(path: &[EntryRow]) -> Result<SessionContext> {
    if path.is_empty() {
        return Ok(SessionContext {
            messages: Vec::new(),
            thinking_level: bb_core::types::ThinkingLevel::Off,
            model: None,
        });
    }

    let entries: Vec<SessionEntry> = path
        .iter()
        .map(store::parse_entry)
        .collect::<Result<Vec<_>>>()?;

    Ok(assembly::build_context_from_entries(&entries))
}
