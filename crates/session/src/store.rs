use anyhow::Result;
use rusqlite::Connection;

mod fork;
mod queries;
#[cfg(test)]
mod tests;
mod writes;

/// A lightweight row from the entries table.
#[derive(Clone, Debug, PartialEq, Eq)]
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
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionRow {
    pub session_id: String,
    pub cwd: String,
    pub created_at: String,
    pub updated_at: String,
    pub name: Option<String>,
    pub leaf_id: Option<String>,
    pub entry_count: i64,
    pub parent_session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForkSessionResult {
    pub session_id: String,
    pub selected_text: String,
    pub branch_leaf_id: Option<String>,
}

pub fn open_db(path: &std::path::Path) -> Result<Connection> {
    writes::open_db(path)
}

pub fn open_memory() -> Result<Connection> {
    writes::open_memory()
}

pub fn create_session(conn: &Connection, cwd: &str) -> Result<String> {
    writes::create_session(conn, cwd)
}

pub fn create_session_with_parent(
    conn: &Connection,
    cwd: &str,
    parent_session_id: Option<&str>,
) -> Result<String> {
    writes::create_session_with_parent(conn, cwd, parent_session_id)
}

pub fn create_session_with_id(conn: &Connection, session_id: &str, cwd: &str) -> Result<()> {
    writes::create_session_with_id(conn, session_id, cwd)
}

pub fn create_session_with_id_and_parent(
    conn: &Connection,
    session_id: &str,
    cwd: &str,
    parent_session_id: Option<&str>,
) -> Result<()> {
    writes::create_session_with_id_and_parent(conn, session_id, cwd, parent_session_id)
}

pub fn append_entry(
    conn: &Connection,
    session_id: &str,
    entry: &bb_core::types::SessionEntry,
) -> Result<i64> {
    writes::append_entry(conn, session_id, entry)
}

pub fn get_entry(conn: &Connection, session_id: &str, entry_id: &str) -> Result<Option<EntryRow>> {
    queries::get_entry(conn, session_id, entry_id)
}

pub fn get_entries(conn: &Connection, session_id: &str) -> Result<Vec<EntryRow>> {
    queries::get_entries(conn, session_id)
}

pub fn get_children(conn: &Connection, session_id: &str, parent_id: &str) -> Result<Vec<EntryRow>> {
    queries::get_children(conn, session_id, parent_id)
}

pub fn get_session(conn: &Connection, session_id: &str) -> Result<Option<SessionRow>> {
    queries::get_session(conn, session_id)
}

pub fn list_sessions(conn: &Connection, cwd: &str) -> Result<Vec<SessionRow>> {
    queries::list_sessions(conn, cwd)
}

pub fn set_leaf(conn: &Connection, session_id: &str, leaf_id: Option<&str>) -> Result<()> {
    writes::set_leaf(conn, session_id, leaf_id)
}

pub fn set_session_name(conn: &Connection, session_id: &str, name: Option<&str>) -> Result<()> {
    writes::set_session_name(conn, session_id, name)
}

pub fn parse_entry(row: &EntryRow) -> Result<bb_core::types::SessionEntry> {
    queries::parse_entry(row)
}

pub fn copy_branch_to_session(
    conn: &Connection,
    source_session_id: &str,
    target_session_id: &str,
    leaf_id: &str,
) -> Result<()> {
    fork::copy_branch_to_session(conn, source_session_id, target_session_id, leaf_id)
}

pub fn fork_session_from_entry(
    conn: &Connection,
    source_session_id: &str,
    entry_id: &str,
    cwd: &str,
) -> Result<ForkSessionResult> {
    fork::fork_session_from_entry(conn, source_session_id, entry_id, cwd)
}
