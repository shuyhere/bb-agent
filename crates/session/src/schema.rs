use anyhow::Result;
use rusqlite::Connection;

#[allow(dead_code)]
const CURRENT_VERSION: i32 = 1;

const SCHEMA_V1: &str = r#"
CREATE TABLE IF NOT EXISTS entries (
    session_id TEXT    NOT NULL,
    seq        INTEGER NOT NULL,
    entry_id   TEXT    NOT NULL,
    parent_id  TEXT,
    type       TEXT    NOT NULL,
    timestamp  TEXT    NOT NULL,
    payload    TEXT    NOT NULL,
    PRIMARY KEY (session_id, seq)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_entry_id
    ON entries(session_id, entry_id);

CREATE INDEX IF NOT EXISTS idx_entry_parent
    ON entries(session_id, parent_id);

CREATE TABLE IF NOT EXISTS sessions (
    session_id  TEXT PRIMARY KEY,
    cwd         TEXT    NOT NULL,
    created_at  TEXT    NOT NULL,
    updated_at  TEXT    NOT NULL,
    name        TEXT,
    leaf_id     TEXT,
    entry_count INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_sessions_cwd
    ON sessions(cwd);

CREATE TABLE IF NOT EXISTS schema_version (
    version    INTEGER PRIMARY KEY,
    applied_at TEXT NOT NULL
);
"#;

/// Initialize database schema, applying migrations as needed.
pub fn init_schema(conn: &Connection) -> Result<()> {
    let current = get_version(conn);

    if current < 1 {
        conn.execute_batch(SCHEMA_V1)?;
        set_version(conn, 1)?;
    }

    // Future migrations:
    // if current < 2 { ... set_version(conn, 2)?; }

    Ok(())
}

fn get_version(conn: &Connection) -> i32 {
    // Table may not exist yet
    let result = conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_version",
        [],
        |row| row.get::<_, i32>(0),
    );
    result.unwrap_or(0)
}

fn set_version(conn: &Connection, version: i32) -> Result<()> {
    conn.execute(
        "INSERT INTO schema_version (version, applied_at) VALUES (?1, datetime('now'))",
        rusqlite::params![version],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_schema() {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        assert_eq!(get_version(&conn), 1);

        // Idempotent
        init_schema(&conn).unwrap();
        assert_eq!(get_version(&conn), 1);
    }
}
