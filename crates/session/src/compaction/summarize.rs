use super::{file_ops::extract_file_operations, serialize::serialize_conversation, types::*};
use bb_core::types::{AgentMessage, SessionEntry};
use bb_provider::{CompletionRequest, RequestOptions, StreamEvent};
use tokio_util::sync::CancellationToken;

// =============================================================================
// Summarization prompts
// =============================================================================

pub const SUMMARIZATION_SYSTEM_PROMPT: &str = "You are a conversation summarizer for a coding agent session. \
    Your job is to create a structured context checkpoint that captures all essential information \
    needed to continue the conversation without the original messages. Be precise, concise, and \
    preserve technical details like file paths, function names, and error messages.";

pub const SUMMARIZATION_PROMPT: &str = r#"The messages above are a conversation to summarize.
Create a structured context checkpoint:

## Goal
[What is the user trying to accomplish?]

## Constraints & Preferences
- [Requirements]

## Progress
### Done
- [x] [Completed tasks]
### In Progress
- [ ] [Current work]

## Key Decisions
- **[Decision]**: [Rationale]

## Next Steps
1. [Ordered list]

## Critical Context
- [Data needed to continue]
"#;

// =============================================================================
// Compaction result
// =============================================================================

/// Result of executing compaction.
#[derive(Debug)]
pub struct CompactionResult {
    pub summary: String,
    pub first_kept_entry_id: String,
    pub tokens_before: u64,
    pub read_files: Vec<String>,
    pub modified_files: Vec<String>,
}

// =============================================================================
// Compact execution
// =============================================================================

/// Execute compaction: call LLM to generate summary.
/// Returns CompactionResult with summary text and metadata.
pub async fn compact(
    preparation: &CompactionPreparation,
    provider: &dyn bb_provider::Provider,
    model: &str,
    api_key: &str,
    base_url: &str,
    custom_instructions: Option<&str>,
    cancel: CancellationToken,
) -> anyhow::Result<CompactionResult> {
    // 1. Parse messages from entry rows
    let messages: Vec<AgentMessage> = preparation
        .messages_to_summarize
        .iter()
        .filter_map(
            |entry| match serde_json::from_str::<SessionEntry>(&entry.payload).ok()? {
                SessionEntry::Message { message, .. } => Some(message),
                _ => None,
            },
        )
        .collect();

    // 2. Serialize conversation
    let conversation_text = serialize_conversation(&messages);

    // 3. Build user prompt
    let mut user_prompt = conversation_text;

    // 4. Include previous summary if exists
    if let Some(ref prev) = preparation.previous_summary {
        user_prompt = format!(
            "Previous summary of earlier conversation:\n{prev}\n\n---\n\nNew conversation to summarize:\n{user_prompt}"
        );
    }

    // 5. Append custom instructions
    if let Some(instructions) = custom_instructions {
        user_prompt.push_str(&format!("\n\nAdditional context: {instructions}"));
    }

    user_prompt.push_str("\n\n");
    user_prompt.push_str(SUMMARIZATION_PROMPT);

    // 6. Build request
    let request = CompletionRequest {
        system_prompt: SUMMARIZATION_SYSTEM_PROMPT.to_string(),
        messages: vec![serde_json::json!({
            "role": "user",
            "content": user_prompt
        })],
        tools: vec![],
        extra_tool_schemas: vec![],
        model: model.to_string(),
        max_tokens: Some(4096),
        stream: false,
        thinking: None,
    };

    let options = RequestOptions {
        api_key: api_key.to_string(),
        base_url: base_url.to_string(),
        headers: std::collections::HashMap::new(),
        cancel,
        retry_callback: None,
        max_retries: 1,
        retry_base_delay_ms: 1_000,
        max_retry_delay_ms: 60_000,
    };

    // 7. Call provider
    let events = provider.complete(request, options).await?;

    // 8. Extract summary text from events
    let mut summary = String::new();
    for event in &events {
        if let StreamEvent::TextDelta { text } = event {
            summary.push_str(text);
        }
    }

    if summary.is_empty() {
        anyhow::bail!("Compaction LLM returned empty summary");
    }

    // 9. Extract file operations
    let (read_files, modified_files) = extract_file_operations(&messages);

    // 10. Append file lists to summary
    if !read_files.is_empty() {
        summary.push_str("\n\n<read-files>\n");
        for f in &read_files {
            summary.push_str(&format!("- {f}\n"));
        }
        summary.push_str("</read-files>");
    }
    if !modified_files.is_empty() {
        summary.push_str("\n\n<modified-files>\n");
        for f in &modified_files {
            summary.push_str(&format!("- {f}\n"));
        }
        summary.push_str("</modified-files>");
    }

    Ok(CompactionResult {
        summary,
        first_kept_entry_id: preparation.first_kept_entry_id.clone(),
        tokens_before: preparation.tokens_before,
        read_files,
        modified_files,
    })
}
