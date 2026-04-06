use anyhow::Result;
use bb_core::types::SessionEntry;
use chrono::Utc;
use rusqlite::{Connection, params};
use uuid::Uuid;

use crate::schema;

/// Open or create the sessions database.
pub(super) fn open_db(path: &std::path::Path) -> Result<Connection> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(path)?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
    schema::init_schema(&conn)?;
    Ok(conn)
}

/// Open an in-memory database (for testing).
pub(super) fn open_memory() -> Result<Connection> {
    let conn = Connection::open_in_memory()?;
    schema::init_schema(&conn)?;
    Ok(conn)
}

/// Create a new session.
pub(super) fn create_session(conn: &Connection, cwd: &str) -> Result<String> {
    let session_id = Uuid::new_v4().to_string();
    create_session_with_id_and_parent(conn, &session_id, cwd, None)?;
    Ok(session_id)
}

pub(super) fn create_session_with_parent(
    conn: &Connection,
    cwd: &str,
    parent_session_id: Option<&str>,
) -> Result<String> {
    let session_id = Uuid::new_v4().to_string();
    create_session_with_id_and_parent(conn, &session_id, cwd, parent_session_id)?;
    Ok(session_id)
}

/// Create a session with a specific ID (for lazy creation).
pub(super) fn create_session_with_id(conn: &Connection, session_id: &str, cwd: &str) -> Result<()> {
    create_session_with_id_and_parent(conn, session_id, cwd, None)
}

pub(super) fn create_session_with_id_and_parent(
    conn: &Connection,
    session_id: &str,
    cwd: &str,
    parent_session_id: Option<&str>,
) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO sessions (session_id, cwd, created_at, updated_at, name, leaf_id, entry_count, parent_session_id)
         VALUES (?1, ?2, ?3, ?4, NULL, NULL, 0, ?5)",
        params![session_id, cwd, now, now, parent_session_id],
    )?;
    Ok(())
}

/// Append an entry to a session. Returns the assigned sequence number.
pub(super) fn append_entry(
    conn: &Connection,
    session_id: &str,
    entry: &SessionEntry,
) -> Result<i64> {
    let base = entry.base();
    let entry_type = entry.entry_type();
    let payload = serde_json::to_string(entry)?;
    let timestamp = base.timestamp.to_rfc3339();
    let parent_id = base.parent_id.as_ref().map(|id| id.as_str().to_string());

    let seq: i64 = conn.query_row(
        "SELECT COALESCE(MAX(seq), 0) + 1 FROM entries WHERE session_id = ?1",
        params![session_id],
        |row| row.get(0),
    )?;

    conn.execute(
        "INSERT INTO entries (session_id, seq, entry_id, parent_id, type, timestamp, payload)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            session_id,
            seq,
            base.id.as_str(),
            parent_id,
            entry_type,
            timestamp,
            payload,
        ],
    )?;

    conn.execute(
        "UPDATE sessions SET
            leaf_id = ?1,
            updated_at = ?2,
            entry_count = entry_count + 1
         WHERE session_id = ?3",
        params![base.id.as_str(), timestamp, session_id],
    )?;

    Ok(seq)
}

/// Move the leaf pointer to an earlier entry (branching).
pub(super) fn set_leaf(conn: &Connection, session_id: &str, leaf_id: Option<&str>) -> Result<()> {
    conn.execute(
        "UPDATE sessions SET leaf_id = ?1, updated_at = datetime('now')
         WHERE session_id = ?2",
        params![leaf_id, session_id],
    )?;
    Ok(())
}

/// Set or clear the display name for a session.
pub(super) fn set_session_name(
    conn: &Connection,
    session_id: &str,
    name: Option<&str>,
) -> Result<()> {
    conn.execute(
        "UPDATE sessions SET name = ?1, updated_at = datetime('now') WHERE session_id = ?2",
        params![name, session_id],
    )?;
    Ok(())
}
