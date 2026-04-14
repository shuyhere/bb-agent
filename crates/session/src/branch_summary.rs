use std::collections::HashSet;

use anyhow::Result;
use bb_core::types::{AgentMessage, SessionEntry};
use bb_provider::{CompletionRequest, Provider, RequestOptions, StreamEvent};
use tokio_util::sync::CancellationToken;

use crate::compaction::{extract_file_operations, serialize_conversation};
use crate::store::EntryRow;

pub const BRANCH_SUMMARY_PREAMBLE: &str = "The user explored a different conversation branch before returning here.\nSummary of that exploration:\n\n";

pub const BRANCH_SUMMARY_SYSTEM_PROMPT: &str = "You are a conversation summarizer for a coding agent session. Your job is to create a structured context checkpoint that captures all essential information needed to continue the conversation without the original messages. Be precise, concise, and preserve technical details like file paths, function names, and error messages.";

pub const BRANCH_SUMMARY_PROMPT: &str = r#"Create a structured summary of this conversation branch for context when returning later.

Use this EXACT format:

## Goal
[What was the user trying to accomplish in this branch?]

## Constraints & Preferences
- [Any constraints, preferences, or requirements mentioned]
- [Or "(none)" if none were mentioned]

## Progress
### Done
- [x] [Completed tasks/changes]

### In Progress
- [ ] [Work that was started but not finished]

### Blocked
- [Issues preventing progress, if any]

## Key Decisions
- **[Decision]**: [Brief rationale]

## Next Steps
1. [What should happen next to continue this work]

Keep each section concise. Preserve exact file paths, function names, and error messages."#;

#[derive(Debug, Clone)]
pub struct BranchSummaryPreparation {
    pub messages: Vec<AgentMessage>,
    pub read_files: Vec<String>,
    pub modified_files: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct BranchSummaryResult {
    pub summary: String,
    pub read_files: Vec<String>,
    pub modified_files: Vec<String>,
}

pub fn prepare_branch_summary_entries(rows: &[EntryRow]) -> Result<BranchSummaryPreparation> {
    let mut messages = Vec::new();
    let mut nested_read_files = HashSet::new();
    let mut nested_modified_files = HashSet::new();

    for row in rows {
        let entry: SessionEntry = serde_json::from_str(&row.payload)?;
        match entry {
            SessionEntry::Message { message, .. } => {
                if !matches!(message, AgentMessage::ToolResult(_)) {
                    messages.push(message);
                }
            }
            SessionEntry::CustomMessage {
                custom_type,
                content,
                display,
                details,
                ..
            } => messages.push(AgentMessage::Custom(bb_core::types::CustomMessage {
                custom_type,
                content,
                display,
                details,
                timestamp: chrono::Utc::now().timestamp_millis(),
            })),
            SessionEntry::BranchSummary {
                summary,
                from_id,
                details,
                ..
            } => {
                if let Some(details) = details {
                    if let Some(files) =
                        details.get("read_files").and_then(|value| value.as_array())
                    {
                        for value in files.iter().filter_map(|value| value.as_str()) {
                            nested_read_files.insert(value.to_string());
                        }
                    }
                    if let Some(files) = details
                        .get("modified_files")
                        .and_then(|value| value.as_array())
                    {
                        for value in files.iter().filter_map(|value| value.as_str()) {
                            nested_modified_files.insert(value.to_string());
                        }
                    }
                }
                messages.push(AgentMessage::BranchSummary(
                    bb_core::types::BranchSummaryMessage {
                        summary,
                        from_id: from_id.0,
                        timestamp: chrono::Utc::now().timestamp_millis(),
                    },
                ));
            }
            SessionEntry::Compaction {
                summary,
                tokens_before,
                ..
            } => messages.push(AgentMessage::CompactionSummary(
                bb_core::types::CompactionSummaryMessage {
                    summary,
                    tokens_before,
                    timestamp: chrono::Utc::now().timestamp_millis(),
                },
            )),
            _ => {}
        }
    }

    let (mut read_files, mut modified_files) = extract_file_operations(&messages);
    for path in nested_read_files {
        read_files.push(path);
    }
    for path in nested_modified_files {
        modified_files.push(path);
    }
    read_files.sort();
    read_files.dedup();
    modified_files.sort();
    modified_files.dedup();

    Ok(BranchSummaryPreparation {
        messages,
        read_files,
        modified_files,
    })
}

/// Inputs required to summarize an abandoned branch without relying on positional arguments that
/// are easy to mix up at the call site.
pub struct BranchSummaryRequest<'a> {
    pub rows: &'a [EntryRow],
    pub provider: &'a dyn Provider,
    pub model: &'a str,
    pub api_key: &'a str,
    pub base_url: &'a str,
    pub custom_instructions: Option<&'a str>,
    pub replace_instructions: bool,
    pub cancel: CancellationToken,
}

pub async fn generate_branch_summary(
    request: BranchSummaryRequest<'_>,
) -> Result<BranchSummaryResult> {
    let BranchSummaryRequest {
        rows,
        provider,
        model,
        api_key,
        base_url,
        custom_instructions,
        replace_instructions,
        cancel,
    } = request;
    let preparation = prepare_branch_summary_entries(rows)?;
    if preparation.messages.is_empty() {
        return Ok(BranchSummaryResult {
            summary: format!("{BRANCH_SUMMARY_PREAMBLE}No content to summarize"),
            read_files: preparation.read_files,
            modified_files: preparation.modified_files,
        });
    }

    let conversation_text = serialize_conversation(&preparation.messages);
    let instructions = if replace_instructions {
        custom_instructions
            .unwrap_or(BRANCH_SUMMARY_PROMPT)
            .to_string()
    } else if let Some(custom) = custom_instructions.filter(|text| !text.trim().is_empty()) {
        format!("{BRANCH_SUMMARY_PROMPT}\n\nAdditional focus: {custom}")
    } else {
        BRANCH_SUMMARY_PROMPT.to_string()
    };
    let user_prompt =
        format!("<conversation>\n{conversation_text}\n</conversation>\n\n{instructions}");

    let request = CompletionRequest {
        system_prompt: BRANCH_SUMMARY_SYSTEM_PROMPT.to_string(),
        messages: vec![serde_json::json!({
            "role": "user",
            "content": user_prompt,
        })],
        tools: vec![],
        extra_tool_schemas: vec![],
        model: model.to_string(),
        max_tokens: Some(2048),
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

    let events = provider
        .complete(request, options)
        .await
        .map_err(|err| anyhow::anyhow!(err.to_string()))?;

    let mut summary = String::new();
    for event in &events {
        if let StreamEvent::TextDelta { text } = event {
            summary.push_str(text);
        }
    }
    if summary.trim().is_empty() {
        anyhow::bail!("Branch summarization LLM returned empty summary");
    }

    summary = format!("{BRANCH_SUMMARY_PREAMBLE}{summary}");
    append_file_lists(
        &mut summary,
        &preparation.read_files,
        &preparation.modified_files,
    );

    Ok(BranchSummaryResult {
        summary,
        read_files: preparation.read_files,
        modified_files: preparation.modified_files,
    })
}

fn append_file_lists(summary: &mut String, read_files: &[String], modified_files: &[String]) {
    if !read_files.is_empty() {
        summary.push_str("\n\n<read-files>\n");
        for file in read_files {
            summary.push_str(&format!("- {file}\n"));
        }
        summary.push_str("</read-files>");
    }
    if !modified_files.is_empty() {
        summary.push_str("\n\n<modified-files>\n");
        for file in modified_files {
            summary.push_str(&format!("- {file}\n"));
        }
        summary.push_str("</modified-files>");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bb_core::types::{EntryBase, EntryId, UserMessage};
    use chrono::Utc;

    #[test]
    fn prompt_includes_blocked_section() {
        assert!(BRANCH_SUMMARY_PROMPT.contains("### Blocked"));
    }

    #[test]
    fn prepare_branch_summary_extracts_files() {
        let entry = SessionEntry::Message {
            base: EntryBase {
                id: EntryId::generate(),
                parent_id: None,
                timestamp: Utc::now(),
            },
            message: AgentMessage::User(UserMessage {
                content: vec![bb_core::types::ContentBlock::Text {
                    text: "hello".into(),
                }],
                timestamp: Utc::now().timestamp_millis(),
            }),
        };
        let row = EntryRow {
            session_id: "s".into(),
            seq: 1,
            entry_id: entry.base().id.0.clone(),
            parent_id: None,
            entry_type: entry.entry_type().to_string(),
            timestamp: entry.base().timestamp.to_rfc3339(),
            payload: serde_json::to_string(&entry).unwrap(),
        };
        let prep = prepare_branch_summary_entries(&[row]).unwrap();
        assert_eq!(prep.messages.len(), 1);
        assert!(prep.read_files.is_empty());
        assert!(prep.modified_files.is_empty());
    }
}
