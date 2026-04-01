mod file_ops;
mod planning;
mod serialize;
mod summarize;
mod types;

pub use file_ops::extract_file_operations;
pub use planning::{estimate_tokens_row, estimate_tokens_text, find_cut_point, prepare_compaction, should_compact};
pub use serialize::serialize_conversation;
pub use summarize::{compact, SUMMARIZATION_PROMPT, SUMMARIZATION_SYSTEM_PROMPT};
pub use types::{CompactionPreparation, CompactionResult};

#[cfg(test)]
mod tests;
