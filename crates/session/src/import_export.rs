use anyhow::Result;
use bb_core::types::SessionEntry;
use rusqlite::Connection;
use std::io::{BufRead, BufReader};
use std::path::Path;

use crate::store;

/// Import a pi-format JSONL session file into SQLite.
pub fn import_jsonl(path: &Path, conn: &Connection) -> Result<String> {
    let file = std::fs::File::open(path)?;
    let reader = BufReader::new(file);
    let mut lines = reader.lines();

    // First line is session header
    let header_line = lines
        .next()
        .ok_or_else(|| anyhow::anyhow!("Empty session file"))??;
    let header: serde_json::Value = serde_json::from_str(&header_line)?;

    let cwd = header
        .get("cwd")
        .and_then(|v| v.as_str())
        .unwrap_or(".")
        .to_string();

    let session_id = store::create_session(conn, &cwd)?;

    // Remaining lines are entries
    for line_result in lines {
        let line = line_result?;
        if line.trim().is_empty() {
            continue;
        }

        let entry: SessionEntry = match serde_json::from_str(&line) {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!("Skipping unparseable entry: {e}");
                continue;
            }
        };

        store::append_entry(conn, &session_id, &entry)?;
    }

    Ok(session_id)
}

/// Export a session from SQLite to pi-compatible JSONL.
pub fn export_jsonl(conn: &Connection, session_id: &str, output: &Path) -> Result<()> {
    use std::io::Write;

    let session = store::get_session(conn, session_id)?
        .ok_or_else(|| anyhow::anyhow!("Session not found: {session_id}"))?;

    let mut file = std::fs::File::create(output)?;

    // Write header
    let header = serde_json::json!({
        "type": "session",
        "version": 3,
        "id": session_id,
        "timestamp": session.created_at,
        "cwd": session.cwd,
    });
    writeln!(file, "{}", serde_json::to_string(&header)?)?;

    // Write entries
    let entries = store::get_entries(conn, session_id)?;
    for entry in &entries {
        writeln!(file, "{}", entry.payload)?;
    }

    Ok(())
}
