mod file_ops;
mod planning;
mod serialize;
mod summarize;
mod types;

pub use file_ops::extract_file_operations;
pub use planning::{
    ContextUsageEstimate, calculate_context_tokens, estimate_context_tokens,
    estimate_tokens_message, estimate_tokens_row, estimate_tokens_text, find_cut_point,
    prepare_compaction, should_compact,
};
pub use serialize::serialize_conversation;
pub use summarize::{
    CompactionRequest, SUMMARIZATION_PROMPT, SUMMARIZATION_SYSTEM_PROMPT, compact,
};
pub use types::{CompactionPreparation, CompactionResult};

#[cfg(test)]
mod tests;
