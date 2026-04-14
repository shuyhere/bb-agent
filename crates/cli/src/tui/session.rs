use anyhow::Result;
use bb_tui::tui::Transcript;

mod compaction;
mod db;
mod export;
mod resume;
mod transcript;
mod tree_actions;

const HIDDEN_DISPATCH_PREFIX: &str = "[[bb-hidden-dispatch]]\n";

pub(super) fn build_tui_transcript(
    conn: &rusqlite::Connection,
    session_id: &str,
) -> Result<(
    Transcript,
    std::collections::HashMap<String, bb_tui::tui::HistoricalToolState>,
)> {
    transcript::build_tui_transcript(conn, session_id)
}

pub(super) fn export_session(
    conn: &rusqlite::Connection,
    session_id: &str,
    file_path: &str,
) -> anyhow::Result<String> {
    export::export_session(conn, session_id, file_path)
}

#[cfg(test)]
fn truncate_preview_text(text: &str, max_chars: usize) -> String {
    transcript::truncate_preview_text(text, max_chars)
}

#[cfg(test)]
mod tests;
