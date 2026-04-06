use anyhow::Result;
use bb_core::types::SessionEntry;
use rusqlite::{Connection, params};

use super::{EntryRow, SessionRow};

/// Get an entry by id.
pub(super) fn get_entry(
    conn: &Connection,
    session_id: &str,
    entry_id: &str,
) -> Result<Option<EntryRow>> {
    let mut stmt = conn.prepare(
        "SELECT session_id, seq, entry_id, parent_id, type, timestamp, payload
         FROM entries WHERE session_id = ?1 AND entry_id = ?2",
    )?;
    let row = stmt.query_row(params![session_id, entry_id], |row| {
        Ok(EntryRow {
            session_id: row.get(0)?,
            seq: row.get(1)?,
            entry_id: row.get(2)?,
            parent_id: row.get(3)?,
            entry_type: row.get(4)?,
            timestamp: row.get(5)?,
            payload: row.get(6)?,
        })
    });

    match row {
        Ok(r) => Ok(Some(r)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Get all entries for a session, ordered by sequence.
pub(super) fn get_entries(conn: &Connection, session_id: &str) -> Result<Vec<EntryRow>> {
    let mut stmt = conn.prepare(
        "SELECT session_id, seq, entry_id, parent_id, type, timestamp, payload
         FROM entries WHERE session_id = ?1 ORDER BY seq",
    )?;
    let rows = stmt.query_map(params![session_id], |row| {
        Ok(EntryRow {
            session_id: row.get(0)?,
            seq: row.get(1)?,
            entry_id: row.get(2)?,
            parent_id: row.get(3)?,
            entry_type: row.get(4)?,
            timestamp: row.get(5)?,
            payload: row.get(6)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// Get children of an entry, sorted by timestamp.
pub(super) fn get_children(
    conn: &Connection,
    session_id: &str,
    parent_id: &str,
) -> Result<Vec<EntryRow>> {
    let mut stmt = conn.prepare(
        "SELECT session_id, seq, entry_id, parent_id, type, timestamp, payload
         FROM entries WHERE session_id = ?1 AND parent_id = ?2
         ORDER BY timestamp",
    )?;
    let rows = stmt.query_map(params![session_id, parent_id], |row| {
        Ok(EntryRow {
            session_id: row.get(0)?,
            seq: row.get(1)?,
            entry_id: row.get(2)?,
            parent_id: row.get(3)?,
            entry_type: row.get(4)?,
            timestamp: row.get(5)?,
            payload: row.get(6)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// Get session metadata.
pub(super) fn get_session(conn: &Connection, session_id: &str) -> Result<Option<SessionRow>> {
    let mut stmt = conn.prepare(
        "SELECT session_id, cwd, created_at, updated_at, name, leaf_id, entry_count, parent_session_id
         FROM sessions WHERE session_id = ?1",
    )?;
    let row = stmt.query_row(params![session_id], |row| {
        Ok(SessionRow {
            session_id: row.get(0)?,
            cwd: row.get(1)?,
            created_at: row.get(2)?,
            updated_at: row.get(3)?,
            name: row.get(4)?,
            leaf_id: row.get(5)?,
            entry_count: row.get(6)?,
            parent_session_id: row.get(7)?,
        })
    });

    match row {
        Ok(r) => Ok(Some(r)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// List sessions for a given cwd, most recent first.
pub(super) fn list_sessions(conn: &Connection, cwd: &str) -> Result<Vec<SessionRow>> {
    let mut stmt = conn.prepare(
        "SELECT session_id, cwd, created_at, updated_at, name, leaf_id, entry_count, parent_session_id
         FROM sessions WHERE cwd = ?1 ORDER BY updated_at DESC",
    )?;
    let rows = stmt.query_map(params![cwd], |row| {
        Ok(SessionRow {
            session_id: row.get(0)?,
            cwd: row.get(1)?,
            created_at: row.get(2)?,
            updated_at: row.get(3)?,
            name: row.get(4)?,
            leaf_id: row.get(5)?,
            entry_count: row.get(6)?,
            parent_session_id: row.get(7)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// Parse an EntryRow payload into a SessionEntry.
pub(super) fn parse_entry(row: &EntryRow) -> Result<SessionEntry> {
    let entry: SessionEntry = serde_json::from_str(&row.payload)?;
    Ok(entry)
}
