use anyhow::Result;
use bb_core::types::SessionEntry;
use rusqlite::Connection;
use std::io::{BufRead, BufReader};
use std::path::Path;

use crate::store;

/// Import a legacy BB-Agent JSONL session file into SQLite.
pub(super) fn import_jsonl(path: &Path, conn: &Connection) -> Result<String> {
    let file = std::fs::File::open(path)?;
    let reader = BufReader::new(file);
    let mut lines = reader.lines();

    let header_line = lines
        .next()
        .ok_or_else(|| anyhow::anyhow!("Empty session file"))??;
    let header: serde_json::Value = serde_json::from_str(&header_line)?;

    let cwd = header
        .get("cwd")
        .and_then(|v| v.as_str())
        .unwrap_or(".")
        .to_string();

    let parent_session = header
        .get("parent_session")
        .or_else(|| header.get("parentSession"))
        .and_then(|v| v.as_str());

    let session_id = store::create_session_with_parent(conn, &cwd, parent_session)?;

    for line_result in lines {
        let line = line_result?;
        if line.trim().is_empty() {
            continue;
        }

        let entry: SessionEntry = match serde_json::from_str(&line) {
            Ok(entry) => entry,
            Err(error) => {
                tracing::warn!("Skipping unparseable entry: {error}");
                continue;
            }
        };

        store::append_entry(conn, &session_id, &entry)?;
    }

    Ok(session_id)
}
