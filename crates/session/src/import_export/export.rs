use anyhow::Result;
use rusqlite::Connection;
use std::path::Path;

use crate::store;

/// Export a session from SQLite to legacy BB-Agent-compatible JSONL.
pub(super) fn export_jsonl(conn: &Connection, session_id: &str, output: &Path) -> Result<()> {
    use std::io::Write;

    let session = store::get_session(conn, session_id)?
        .ok_or_else(|| anyhow::anyhow!("Session not found: {session_id}"))?;

    let mut file = std::fs::File::create(output)?;

    let header = serde_json::json!({
        "type": "session",
        "version": 3,
        "id": session_id,
        "timestamp": session.created_at,
        "cwd": session.cwd,
        "parent_session": session.parent_session_id,
    });
    writeln!(file, "{}", serde_json::to_string(&header)?)?;

    let entries = store::get_entries(conn, session_id)?;
    for entry in &entries {
        writeln!(file, "{}", entry.payload)?;
    }

    Ok(())
}
