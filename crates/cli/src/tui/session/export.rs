use bb_session::store;

/// Export session entries to a JSONL file. Returns the absolute path.
pub(super) fn export_session(
    conn: &rusqlite::Connection,
    session_id: &str,
    file_path: &str,
) -> anyhow::Result<String> {
    let rows = store::get_entries(conn, session_id)?;
    let mut lines = Vec::new();
    for row in &rows {
        if let Ok(entry) = store::parse_entry(row)
            && let Ok(json) = serde_json::to_string(&entry)
        {
            lines.push(json);
        }
    }
    std::fs::write(file_path, format!("{}\n", lines.join("\n")))?;
    let abs =
        std::fs::canonicalize(file_path).unwrap_or_else(|_| std::path::PathBuf::from(file_path));
    Ok(abs.display().to_string())
}
