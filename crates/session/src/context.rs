use anyhow::Result;
use bb_core::types::{SessionContext, SessionEntry};
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
