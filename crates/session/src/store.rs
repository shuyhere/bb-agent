use anyhow::Result;
use bb_core::types::SessionEntry;
use chrono::Utc;
use rusqlite::{params, Connection};
use uuid::Uuid;

use crate::schema;

/// A lightweight row from the entries table.
#[derive(Clone, Debug)]
pub struct EntryRow {
    pub session_id: String,
    pub seq: i64,
    pub entry_id: String,
    pub parent_id: Option<String>,
    pub entry_type: String,
    pub timestamp: String,
    pub payload: String,
}

/// Session metadata row.
#[derive(Clone, Debug)]
pub struct SessionRow {
    pub session_id: String,
    pub cwd: String,
    pub created_at: String,
    pub updated_at: String,
    pub name: Option<String>,
    pub leaf_id: Option<String>,
    pub entry_count: i64,
}

/// Open or create the sessions database.
pub fn open_db(path: &std::path::Path) -> Result<Connection> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(path)?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
    schema::init_schema(&conn)?;
    Ok(conn)
}

/// Open an in-memory database (for testing).
pub fn open_memory() -> Result<Connection> {
    let conn = Connection::open_in_memory()?;
    schema::init_schema(&conn)?;
    Ok(conn)
}

/// Create a new session.
pub fn create_session(conn: &Connection, cwd: &str) -> Result<String> {
    let session_id = Uuid::new_v4().to_string();
    create_session_with_id(conn, &session_id, cwd)?;
    Ok(session_id)
}

/// Create a session with a specific ID (for lazy creation).
pub fn create_session_with_id(conn: &Connection, session_id: &str, cwd: &str) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO sessions (session_id, cwd, created_at, updated_at, entry_count)
         VALUES (?1, ?2, ?3, ?4, 0)",
        params![session_id, cwd, now, now],
    )?;
    Ok(())
}

/// Append an entry to a session. Returns the assigned sequence number.
pub fn append_entry(
    conn: &Connection,
    session_id: &str,
    entry: &SessionEntry,
) -> Result<i64> {
    let base = entry.base();
    let entry_type = entry.entry_type();
    let payload = serde_json::to_string(entry)?;
    let timestamp = base.timestamp.to_rfc3339();
    let parent_id = base.parent_id.as_ref().map(|id| id.as_str().to_string());

    // Get next sequence number
    let seq: i64 = conn
        .query_row(
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

    // Update session metadata
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

/// Get an entry by id.
pub fn get_entry(conn: &Connection, session_id: &str, entry_id: &str) -> Result<Option<EntryRow>> {
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
pub fn get_entries(conn: &Connection, session_id: &str) -> Result<Vec<EntryRow>> {
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
pub fn get_children(conn: &Connection, session_id: &str, parent_id: &str) -> Result<Vec<EntryRow>> {
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
pub fn get_session(conn: &Connection, session_id: &str) -> Result<Option<SessionRow>> {
    let mut stmt = conn.prepare(
        "SELECT session_id, cwd, created_at, updated_at, name, leaf_id, entry_count
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
        })
    });

    match row {
        Ok(r) => Ok(Some(r)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// List sessions for a given cwd, most recent first.
pub fn list_sessions(conn: &Connection, cwd: &str) -> Result<Vec<SessionRow>> {
    let mut stmt = conn.prepare(
        "SELECT session_id, cwd, created_at, updated_at, name, leaf_id, entry_count
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
        })
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// Move the leaf pointer to an earlier entry (branching).
pub fn set_leaf(conn: &Connection, session_id: &str, leaf_id: Option<&str>) -> Result<()> {
    conn.execute(
        "UPDATE sessions SET leaf_id = ?1, updated_at = datetime('now')
         WHERE session_id = ?2",
        params![leaf_id, session_id],
    )?;
    Ok(())
}

/// Set or clear the display name for a session.
pub fn set_session_name(conn: &Connection, session_id: &str, name: Option<&str>) -> Result<()> {
    conn.execute(
        "UPDATE sessions SET name = ?1, updated_at = datetime('now') WHERE session_id = ?2",
        params![name, session_id],
    )?;
    Ok(())
}

/// Parse an EntryRow payload into a SessionEntry.
pub fn parse_entry(row: &EntryRow) -> Result<SessionEntry> {
    let entry: SessionEntry = serde_json::from_str(&row.payload)?;
    Ok(entry)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bb_core::types::*;
    use chrono::Utc;

    fn make_user_entry(parent: Option<&str>) -> SessionEntry {
        SessionEntry::Message {
            base: EntryBase {
                id: EntryId::generate(),
                parent_id: parent.map(|s| EntryId(s.to_string())),
                timestamp: Utc::now(),
            },
            message: AgentMessage::User(UserMessage {
                content: vec![ContentBlock::Text {
                    text: "hello".to_string(),
                }],
                timestamp: Utc::now().timestamp_millis(),
            }),
        }
    }

    #[test]
    fn test_create_and_append() {
        let conn = open_memory().unwrap();
        let sid = create_session(&conn, "/tmp/test").unwrap();

        let entry = make_user_entry(None);
        let seq = append_entry(&conn, &sid, &entry).unwrap();
        assert_eq!(seq, 1);

        let entries = get_entries(&conn, &sid).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].entry_type, "message");

        let session = get_session(&conn, &sid).unwrap().unwrap();
        assert_eq!(session.entry_count, 1);
        assert!(session.leaf_id.is_some());
    }

    #[test]
    fn test_branching() {
        let conn = open_memory().unwrap();
        let sid = create_session(&conn, "/tmp/test").unwrap();

        let e1 = make_user_entry(None);
        let e1_id = e1.base().id.clone();
        append_entry(&conn, &sid, &e1).unwrap();

        let e2 = make_user_entry(Some(e1_id.as_str()));
        append_entry(&conn, &sid, &e2).unwrap();

        // Branch back to e1
        set_leaf(&conn, &sid, Some(e1_id.as_str())).unwrap();
        let session = get_session(&conn, &sid).unwrap().unwrap();
        assert_eq!(session.leaf_id.as_deref(), Some(e1_id.as_str()));

        // Append creates a new branch
        let e3 = make_user_entry(Some(e1_id.as_str()));
        append_entry(&conn, &sid, &e3).unwrap();

        let children = get_children(&conn, &sid, e1_id.as_str()).unwrap();
        assert_eq!(children.len(), 2); // e2 and e3
    }
}
