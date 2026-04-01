use std::collections::HashSet;

use bb_core::types::{AgentMessage, AssistantContent, CompactionSettings, ContentBlock};
use bb_provider::{CompletionRequest, RequestOptions, StreamEvent};
use tokio_util::sync::CancellationToken;

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

/// Whether compaction should trigger.
pub fn should_compact(context_tokens: u64, context_window: u64, settings: &CompactionSettings) -> bool {
    settings.enabled && context_tokens > context_window.saturating_sub(settings.reserve_tokens)
}

/// Estimate tokens for a message (rough: ~4 chars per token).
pub fn estimate_tokens_text(text: &str) -> u64 {
    (text.len() as u64) / 4
}

/// Estimate tokens for an entry row by its payload size.
pub fn estimate_tokens_row(row: &EntryRow) -> u64 {
    estimate_tokens_text(&row.payload)
}

/// Find the cut point that keeps approximately `keep_recent_tokens`.
///
/// Walks backward from the newest entry, accumulating token estimates.
/// Returns the index of the first entry to keep.
pub fn find_cut_point(
    entries: &[EntryRow],
    start: usize,
    end: usize,
    keep_recent_tokens: u64,
) -> usize {
    let mut accumulated: u64 = 0;
    let mut cut = start;

    for i in (start..end).rev() {
        let entry = &entries[i];
        if entry.entry_type != "message" {
            continue;
        }
        let tokens = estimate_tokens_row(entry);
        accumulated += tokens;

        if accumulated >= keep_recent_tokens {
            // Find valid cut point at or after this index
            cut = find_valid_cut_at_or_after(entries, i, start, end);
            break;
        }
    }

    cut
}

/// Find the nearest valid cut point at or after `idx`.
/// Valid: user message, assistant message, bash execution. Not: tool result.
fn find_valid_cut_at_or_after(entries: &[EntryRow], idx: usize, start: usize, end: usize) -> usize {
    for i in idx..end {
        let entry = &entries[i];
        if entry.entry_type != "message" {
            continue;
        }
        // Parse role from payload (lightweight check)
        if is_valid_cut_role(&entry.payload) {
            return i;
        }
    }
    // Fallback: start of range
    start
}

/// Check if the message role allows cutting here.
fn is_valid_cut_role(payload: &str) -> bool {
    // Quick check without full parse
    payload.contains("\"role\":\"user\"")
        || payload.contains("\"role\":\"assistant\"")
        || payload.contains("\"role\":\"bashExecution\"")
        || payload.contains("\"role\":\"custom\"")
        || payload.contains("\"role\":\"branchSummary\"")
}

/// Prepare compaction data from the active path entries.
pub fn prepare_compaction(
    path_entries: &[EntryRow],
    settings: &CompactionSettings,
) -> Option<CompactionPreparation> {
    if path_entries.is_empty() {
        return None;
    }

    // Don't compact right after a compaction
    if path_entries.last().map(|e| e.entry_type.as_str()) == Some("compaction") {
        return None;
    }

    // Find previous compaction
    let prev_compaction_idx = path_entries
        .iter()
        .enumerate()
        .rev()
        .find(|(_, e)| e.entry_type == "compaction")
        .map(|(i, _)| i);

    let mut previous_summary = None;
    let boundary_start = if let Some(pc_idx) = prev_compaction_idx {
        previous_summary = extract_summary(&path_entries[pc_idx]);
        let first_kept = extract_first_kept_id(&path_entries[pc_idx]);
        if let Some(fk) = first_kept {
            path_entries.iter().position(|e| e.entry_id == fk).unwrap_or(pc_idx + 1)
        } else {
            pc_idx + 1
        }
    } else {
        0
    };

    let boundary_end = path_entries.len();

    // Estimate current context tokens
    let tokens_before: u64 = path_entries.iter().map(estimate_tokens_row).sum();

    // Find cut point
    let cut = find_cut_point(path_entries, boundary_start, boundary_end, settings.keep_recent_tokens);

    if cut <= boundary_start {
        return None; // Nothing to summarize
    }

    let first_kept_entry = &path_entries[cut];

    let messages_to_summarize = path_entries[boundary_start..cut].to_vec();
    let kept_messages = path_entries[cut..].to_vec();

    Some(CompactionPreparation {
        first_kept_entry_id: first_kept_entry.entry_id.clone(),
        messages_to_summarize,
        kept_messages,
        tokens_before,
        previous_summary,
        is_split_turn: false, // Simplified; full split-turn logic added later
    })
}

fn extract_summary(row: &EntryRow) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(&row.payload).ok()?;
    v.get("summary").and_then(|s| s.as_str()).map(|s| s.to_string())
}

fn extract_first_kept_id(row: &EntryRow) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(&row.payload).ok()?;
    v.get("first_kept_entry_id")
        .and_then(|s| s.as_str())
        .map(|s| s.to_string())
}

// =============================================================================
// Conversation serialization
// =============================================================================

/// Serialize messages to text for the summarizer LLM.
/// Format:
///   [User]: message text
///   [Assistant]: response text
///   [Assistant tool calls]: read(path="..."); bash(command="...")
///   [Tool result]: output text (truncated to 2000 chars)
pub fn serialize_conversation(messages: &[AgentMessage]) -> String {
    let mut out = String::new();
    for msg in messages {
        match msg {
            AgentMessage::User(u) => {
                out.push_str("[User]: ");
                for block in &u.content {
                    match block {
                        ContentBlock::Text { text } => out.push_str(text),
                        ContentBlock::Image { .. } => out.push_str("[image]"),
                    }
                }
                out.push('\n');
            }
            AgentMessage::Assistant(a) => {
                let mut text_parts = Vec::new();
                let mut tool_parts = Vec::new();
                for block in &a.content {
                    match block {
                        AssistantContent::Text { text } => text_parts.push(text.clone()),
                        AssistantContent::Thinking { .. } => {}
                        AssistantContent::ToolCall { name, arguments, .. } => {
                            let args_str = format_tool_args(name, arguments);
                            tool_parts.push(format!("{name}({args_str})"));
                        }
                    }
                }
                if !text_parts.is_empty() {
                    out.push_str("[Assistant]: ");
                    out.push_str(&text_parts.join("\n"));
                    out.push('\n');
                }
                if !tool_parts.is_empty() {
                    out.push_str("[Assistant tool calls]: ");
                    out.push_str(&tool_parts.join("; "));
                    out.push('\n');
                }
            }
            AgentMessage::ToolResult(tr) => {
                out.push_str("[Tool result]: ");
                for block in &tr.content {
                    match block {
                        ContentBlock::Text { text } => {
                            if text.len() > 2000 {
                                out.push_str(&text[..2000]);
                                out.push_str("...(truncated)");
                            } else {
                                out.push_str(text);
                            }
                        }
                        ContentBlock::Image { .. } => out.push_str("[image]"),
                    }
                }
                out.push('\n');
            }
            AgentMessage::BashExecution(b) => {
                out.push_str(&format!("[Bash]: {}\n", b.command));
                let output = if b.output.len() > 2000 {
                    format!("{}...(truncated)", &b.output[..2000])
                } else {
                    b.output.clone()
                };
                out.push_str(&format!("[Bash output]: {output}\n"));
            }
            AgentMessage::Custom(c) => {
                out.push_str(&format!("[Custom/{}]: ", c.custom_type));
                for block in &c.content {
                    match block {
                        ContentBlock::Text { text } => out.push_str(text),
                        ContentBlock::Image { .. } => out.push_str("[image]"),
                    }
                }
                out.push('\n');
            }
            AgentMessage::BranchSummary(bs) => {
                out.push_str(&format!("[Branch summary]: {}\n", bs.summary));
            }
            AgentMessage::CompactionSummary(cs) => {
                out.push_str(&format!("[Previous summary]: {}\n", cs.summary));
            }
        }
    }
    out
}

fn format_tool_args(_name: &str, arguments: &serde_json::Value) -> String {
    match arguments.as_object() {
        Some(map) => {
            let pairs: Vec<String> = map
                .iter()
                .map(|(k, v)| {
                    let val = match v.as_str() {
                        Some(s) => {
                            let truncated = if s.len() > 100 { &s[..100] } else { s };
                            format!("\"{truncated}\"")
                        }
                        None => v.to_string(),
                    };
                    format!("{k}={val}")
                })
                .collect();
            pairs.join(", ")
        }
        None => arguments.to_string(),
    }
}

// =============================================================================
// File operation tracking
// =============================================================================

/// Extract read/modified files from messages by looking at tool calls.
pub fn extract_file_operations(messages: &[AgentMessage]) -> (Vec<String>, Vec<String>) {
    let mut read_files = HashSet::new();
    let mut modified_files = HashSet::new();

    for msg in messages {
        match msg {
            AgentMessage::Assistant(a) => {
                for block in &a.content {
                    if let AssistantContent::ToolCall { name, arguments, .. } = block {
                        match name.as_str() {
                            "read" => {
                                if let Some(path) = arguments.get("path").and_then(|v| v.as_str()) {
                                    read_files.insert(path.to_string());
                                }
                            }
                            "edit" | "write" => {
                                if let Some(path) = arguments.get("path").and_then(|v| v.as_str()) {
                                    modified_files.insert(path.to_string());
                                }
                            }
                            "bash" => {
                                if let Some(cmd) = arguments.get("command").and_then(|v| v.as_str()) {
                                    extract_bash_file_ops(cmd, &mut modified_files);
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            _ => {}
        }
    }

    let mut read_vec: Vec<String> = read_files.into_iter().collect();
    let mut mod_vec: Vec<String> = modified_files.into_iter().collect();
    read_vec.sort();
    mod_vec.sort();
    (read_vec, mod_vec)
}

/// Best-effort extraction of modified files from bash commands.
fn extract_bash_file_ops(cmd: &str, modified: &mut HashSet<String>) {
    // Detect redirect operators: > file, >> file
    for part in cmd.split_whitespace() {
        if part.starts_with('>') {
            let file = part.trim_start_matches('>');
            if !file.is_empty() {
                modified.insert(file.to_string());
            }
        }
    }
    // Detect "> file" pattern (space after >)
    let chars: Vec<char> = cmd.chars().collect();
    for i in 0..chars.len() {
        if chars[i] == '>' && (i == 0 || chars[i - 1] != '>') {
            // Skip >> (already handled above for combined token)
            let rest = &cmd[i + 1..];
            let rest = rest.trim_start_matches('>');
            let rest = rest.trim_start();
            if let Some(file) = rest.split_whitespace().next() {
                if !file.is_empty() && !file.starts_with('&') {
                    modified.insert(file.to_string());
                }
            }
        }
    }
    // Detect tee command
    if cmd.contains("tee ") {
        if let Some(pos) = cmd.find("tee ") {
            let after = &cmd[pos + 4..];
            // Skip flags
            for token in after.split_whitespace() {
                if token.starts_with('-') {
                    continue;
                }
                modified.insert(token.to_string());
                break;
            }
        }
    }
}

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
        .filter(|e| e.entry_type == "message")
        .filter_map(|e| serde_json::from_str(&e.payload).ok())
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

#[cfg(test)]
mod tests {
    use super::*;

    use bb_core::types::{
        AssistantMessage, ContentBlock, StopReason, ToolResultMessage, Usage, UserMessage,
    };

    fn make_user_msg(text: &str) -> AgentMessage {
        AgentMessage::User(UserMessage {
            content: vec![ContentBlock::Text {
                text: text.to_string(),
            }],
            timestamp: 0,
        })
    }

    fn make_assistant_msg(text: &str, tool_calls: Vec<(&str, &str, serde_json::Value)>) -> AgentMessage {
        let mut content: Vec<AssistantContent> = Vec::new();
        if !text.is_empty() {
            content.push(AssistantContent::Text {
                text: text.to_string(),
            });
        }
        for (id, name, args) in tool_calls {
            content.push(AssistantContent::ToolCall {
                id: id.to_string(),
                name: name.to_string(),
                arguments: args,
            });
        }
        AgentMessage::Assistant(AssistantMessage {
            content,
            provider: "test".to_string(),
            model: "test".to_string(),
            usage: Usage::default(),
            stop_reason: StopReason::Stop,
            error_message: None,
            timestamp: 0,
        })
    }

    fn make_tool_result(text: &str) -> AgentMessage {
        AgentMessage::ToolResult(ToolResultMessage {
            tool_call_id: "tc1".to_string(),
            tool_name: "read".to_string(),
            content: vec![ContentBlock::Text {
                text: text.to_string(),
            }],
            details: None,
            is_error: false,
            timestamp: 0,
        })
    }

    #[test]
    fn test_serialize_conversation() {
        let messages = vec![
            make_user_msg("Hello, read a file"),
            make_assistant_msg(
                "Sure, let me read it.",
                vec![("tc1", "read", serde_json::json!({"path": "src/main.rs"}))],
            ),
            make_tool_result("fn main() {}"),
        ];

        let serialized = serialize_conversation(&messages);
        assert!(serialized.contains("[User]: Hello, read a file"));
        assert!(serialized.contains("[Assistant]: Sure, let me read it."));
        assert!(serialized.contains("[Assistant tool calls]: read(path=\"src/main.rs\")"));
        assert!(serialized.contains("[Tool result]: fn main() {}"));
    }

    #[test]
    fn test_serialize_conversation_truncates_tool_result() {
        let long_text = "x".repeat(3000);
        let messages = vec![make_tool_result(&long_text)];
        let serialized = serialize_conversation(&messages);
        assert!(serialized.contains("...(truncated)"));
        // Should contain first 2000 chars
        assert!(serialized.contains(&"x".repeat(2000)));
    }

    #[test]
    fn test_extract_file_operations() {
        let messages = vec![
            make_assistant_msg(
                "",
                vec![
                    ("tc1", "read", serde_json::json!({"path": "src/main.rs"})),
                    ("tc2", "edit", serde_json::json!({"path": "src/lib.rs"})),
                    ("tc3", "write", serde_json::json!({"path": "src/new.rs"})),
                    (
                        "tc4",
                        "bash",
                        serde_json::json!({"command": "echo hello > output.txt"}),
                    ),
                ],
            ),
        ];

        let (read, modified) = extract_file_operations(&messages);
        assert_eq!(read, vec!["src/main.rs"]);
        assert!(modified.contains(&"src/lib.rs".to_string()));
        assert!(modified.contains(&"src/new.rs".to_string()));
        assert!(modified.contains(&"output.txt".to_string()));
    }

    #[test]
    fn test_extract_file_operations_deduplicates() {
        let messages = vec![
            make_assistant_msg(
                "",
                vec![
                    ("tc1", "read", serde_json::json!({"path": "src/main.rs"})),
                    ("tc2", "read", serde_json::json!({"path": "src/main.rs"})),
                ],
            ),
        ];
        let (read, _) = extract_file_operations(&messages);
        assert_eq!(read, vec!["src/main.rs"]);
    }

    #[test]
    fn test_summarization_prompt_format() {
        assert!(SUMMARIZATION_PROMPT.contains("## Goal"));
        assert!(SUMMARIZATION_PROMPT.contains("## Constraints & Preferences"));
        assert!(SUMMARIZATION_PROMPT.contains("## Progress"));
        assert!(SUMMARIZATION_PROMPT.contains("### Done"));
        assert!(SUMMARIZATION_PROMPT.contains("### In Progress"));
        assert!(SUMMARIZATION_PROMPT.contains("## Key Decisions"));
        assert!(SUMMARIZATION_PROMPT.contains("## Next Steps"));
        assert!(SUMMARIZATION_PROMPT.contains("## Critical Context"));
    }

    #[test]
    fn test_should_compact() {
        let settings = CompactionSettings::default();
        // 128K context, 100K used — should not compact (100K < 128K - 16K = 112K)
        assert!(!should_compact(100_000, 128_000, &settings));
        // 120K used — should compact (120K > 112K)
        assert!(should_compact(120_000, 128_000, &settings));
    }

    #[test]
    fn test_estimate_tokens() {
        assert_eq!(estimate_tokens_text("hello world"), 2); // 11 chars / 4
        assert_eq!(estimate_tokens_text(""), 0);
    }
}
