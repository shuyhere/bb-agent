use crate::store::EntryRow;

/// Result of compaction preparation.
#[derive(Debug)]
pub struct CompactionPreparation {
    pub first_kept_entry_id: String,
    pub messages_to_summarize: Vec<EntryRow>,
    pub kept_messages: Vec<EntryRow>,
    pub tokens_before: u64,
    pub previous_summary: Option<String>,
    pub is_split_turn: bool,
}

#[derive(Debug)]
pub struct CompactionResult {
    pub summary: String,
    pub first_kept_entry_id: String,
    pub tokens_before: u64,
    pub read_files: Vec<String>,
    pub modified_files: Vec<String>,
}
