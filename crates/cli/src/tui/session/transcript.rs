use std::collections::HashMap;

use anyhow::Result;
use bb_core::types::{AgentMessage, AssistantContent, ContentBlock, SessionEntry, StopReason};
use bb_session::{store, tree};
use bb_tui::tui::{BlockKind, NewBlock, Transcript};

use super::super::formatting::{format_assistant_text, format_user_text, text_from_blocks};
use super::HIDDEN_DISPATCH_PREFIX;

#[cfg(test)]
pub(super) fn truncate_preview_text(text: &str, max_chars: usize) -> String {
    let total_chars = text.chars().count();
    if total_chars <= max_chars {
        return text.to_string();
    }

    let truncated: String = text.chars().take(max_chars).collect();
    format!("{truncated}…")
}

pub(super) fn build_tui_transcript(
    conn: &rusqlite::Connection,
    session_id: &str,
) -> Result<(
    Transcript,
    HashMap<String, bb_tui::tui::HistoricalToolState>,
)> {
    let path = tree::active_path(conn, session_id)?;
    let entries: Vec<SessionEntry> = path.iter().map(store::parse_entry).collect::<Result<_>>()?;

    let mut transcript = Transcript::new();
    let mut tool_map: HashMap<String, bb_tui::tui::BlockId> = HashMap::new();
    let mut tool_states: HashMap<String, bb_tui::tui::HistoricalToolState> = HashMap::new();
    let mut last_assistant_root: Option<bb_tui::tui::BlockId> = None;

    let latest_compaction_idx = entries
        .iter()
        .enumerate()
        .rev()
        .find(|(_, entry)| matches!(entry, SessionEntry::Compaction { .. }))
        .map(|(idx, _)| idx);

    if let Some(compaction_idx) = latest_compaction_idx {
        if let SessionEntry::Compaction {
            first_kept_entry_id,
            ..
        } = &entries[compaction_idx]
        {
            if let Some(first_kept_idx) = entries[..compaction_idx]
                .iter()
                .position(|entry| entry.base().id.as_str() == first_kept_entry_id.as_str())
            {
                for entry in &entries[first_kept_idx..compaction_idx] {
                    append_entry_to_tui_transcript(
                        entry,
                        &mut transcript,
                        &mut tool_map,
                        &mut tool_states,
                        &mut last_assistant_root,
                    )?;
                }
            }

            append_entry_to_tui_transcript(
                &entries[compaction_idx],
                &mut transcript,
                &mut tool_map,
                &mut tool_states,
                &mut last_assistant_root,
            )?;

            for entry in &entries[compaction_idx + 1..] {
                append_entry_to_tui_transcript(
                    entry,
                    &mut transcript,
                    &mut tool_map,
                    &mut tool_states,
                    &mut last_assistant_root,
                )?;
            }
        }
    } else {
        for entry in &entries {
            append_entry_to_tui_transcript(
                entry,
                &mut transcript,
                &mut tool_map,
                &mut tool_states,
                &mut last_assistant_root,
            )?;
        }
    }

    Ok((transcript, tool_states))
}

fn append_entry_to_tui_transcript(
    entry: &SessionEntry,
    transcript: &mut Transcript,
    tool_map: &mut HashMap<String, bb_tui::tui::BlockId>,
    tool_states: &mut HashMap<String, bb_tui::tui::HistoricalToolState>,
    last_assistant_root: &mut Option<bb_tui::tui::BlockId>,
) -> Result<()> {
    match entry {
        SessionEntry::Message { message, .. } => match message {
            AgentMessage::User(user) => {
                let rendered = format_user_text(&user.content);
                if rendered.starts_with(HIDDEN_DISPATCH_PREFIX) {
                    *last_assistant_root = None;
                    return Ok(());
                }
                transcript.append_root_block(
                    NewBlock::new(BlockKind::UserMessage, "prompt").with_content(rendered),
                );
                *last_assistant_root = None;
            }
            AgentMessage::Assistant(message) => {
                let content = format_assistant_text(message);
                let root_id = transcript.append_root_block(
                    NewBlock::new(
                        BlockKind::AssistantMessage,
                        match message.stop_reason {
                            StopReason::Aborted => "aborted",
                            StopReason::Error => "error",
                            _ => "assistant",
                        },
                    )
                    .with_content(content),
                );
                for block in &message.content {
                    match block {
                        AssistantContent::Thinking { thinking } => {
                            let _ = transcript.append_child_block(
                                root_id,
                                NewBlock::new(BlockKind::Thinking, "thinking")
                                    .with_content(thinking.clone()),
                            );
                        }
                        AssistantContent::ToolCall {
                            id,
                            name,
                            arguments,
                        } => {
                            let raw_args = arguments.to_string();
                            let tool_id = transcript.append_child_block(
                                root_id,
                                NewBlock::new(
                                    BlockKind::ToolUse,
                                    bb_tui::tui::format_tool_call_title(name, &raw_args),
                                )
                                .with_content(bb_tui::tui::format_tool_call_content(
                                    name, &raw_args, false,
                                ))
                                .with_expandable(true),
                            )?;
                            tool_map.insert(id.clone(), tool_id);
                            tool_states.insert(
                                id.clone(),
                                bb_tui::tui::HistoricalToolState {
                                    name: name.clone(),
                                    raw_args,
                                    tool_use_id: tool_id,
                                    tool_result_id: None,
                                    result_content: None,
                                    result_details: None,
                                    artifact_path: None,
                                    is_error: false,
                                },
                            );
                        }
                        AssistantContent::Text { .. } => {}
                    }
                }
                *last_assistant_root = Some(root_id);
            }
            AgentMessage::ToolResult(result) => {
                let body = bb_tui::tui::format_tool_result_content(
                    &result.tool_name,
                    &result.content,
                    result.details.clone(),
                    None,
                    result.is_error,
                    false,
                );
                if let Some(tool_use_id) = tool_map.get(&result.tool_call_id).copied() {
                    let tool_result_id = transcript.append_child_block(
                        tool_use_id,
                        NewBlock::new(
                            BlockKind::ToolResult,
                            if result.is_error { "error" } else { "output" },
                        )
                        .with_content(body),
                    )?;
                    if let Some(tool) = tool_states.get_mut(&result.tool_call_id) {
                        tool.tool_result_id = Some(tool_result_id);
                        tool.result_content = Some(result.content.clone());
                        tool.result_details = result.details.clone();
                        tool.is_error = result.is_error;
                    }
                } else if let Some(root_id) = *last_assistant_root {
                    let tool_use_id = transcript.append_child_block(
                        root_id,
                        NewBlock::new(BlockKind::ToolUse, result.tool_name.clone())
                            .with_expandable(true),
                    )?;
                    let _ = transcript.append_child_block(
                        tool_use_id,
                        NewBlock::new(
                            BlockKind::ToolResult,
                            if result.is_error { "error" } else { "output" },
                        )
                        .with_content(body),
                    );
                } else {
                    transcript.append_root_block(
                        NewBlock::new(
                            BlockKind::SystemNote,
                            if result.is_error { "error" } else { "tool" },
                        )
                        .with_content(body),
                    );
                }
            }
            AgentMessage::BashExecution(message) => {
                let raw_args = serde_json::json!({ "command": message.command }).to_string();
                let tool_id = transcript.append_root_block(
                    NewBlock::new(
                        BlockKind::ToolUse,
                        bb_tui::tui::format_tool_call_title("bash", &raw_args),
                    )
                    .with_content(bb_tui::tui::format_tool_call_content(
                        "bash", &raw_args, false,
                    ))
                    .with_expandable(true),
                );
                let output = if message.output.is_empty() {
                    String::new()
                } else {
                    message.output.clone()
                };
                let tool_result_id = transcript.append_child_block(
                    tool_id,
                    NewBlock::new(
                        BlockKind::ToolResult,
                        if message.cancelled {
                            "cancelled"
                        } else {
                            "output"
                        },
                    )
                    .with_content(output),
                )?;
                let historical_id = format!("bash-exec-{}", message.timestamp);
                tool_states.insert(
                    historical_id,
                    bb_tui::tui::HistoricalToolState {
                        name: "bash".to_string(),
                        raw_args,
                        tool_use_id: tool_id,
                        tool_result_id: Some(tool_result_id),
                        result_content: Some(vec![ContentBlock::Text {
                            text: message.output.clone(),
                        }]),
                        result_details: Some(serde_json::json!({
                            "exitCode": message.exit_code,
                            "cancelled": message.cancelled,
                            "truncated": message.truncated,
                        })),
                        artifact_path: message.full_output_path.clone(),
                        is_error: message.cancelled || message.exit_code.unwrap_or_default() != 0,
                    },
                );
                *last_assistant_root = None;
            }
            AgentMessage::Custom(message) => {
                if message.display {
                    transcript.append_root_block(
                        NewBlock::new(BlockKind::SystemNote, message.custom_type.clone())
                            .with_content(text_from_blocks(&message.content, "\n")),
                    );
                }
                *last_assistant_root = None;
            }
            AgentMessage::BranchSummary(message) => {
                transcript.append_root_block(
                    NewBlock::new(BlockKind::SystemNote, "branch summary")
                        .with_content(message.summary.clone()),
                );
                *last_assistant_root = None;
            }
            AgentMessage::CompactionSummary(message) => {
                let content = format!(
                    "[compaction: {} tokens summarized]\n\n{}",
                    message.tokens_before, message.summary
                );
                transcript.append_root_block(
                    NewBlock::new(BlockKind::SystemNote, "compaction")
                        .with_content(content)
                        .with_expandable(true)
                        .with_collapsed(true),
                );
                *last_assistant_root = None;
            }
        },
        SessionEntry::CustomMessage {
            custom_type,
            content,
            display,
            ..
        } => {
            if *display {
                transcript.append_root_block(
                    NewBlock::new(BlockKind::SystemNote, custom_type.clone())
                        .with_content(text_from_blocks(content, "\n")),
                );
            }
            *last_assistant_root = None;
        }
        SessionEntry::BranchSummary { summary, .. } => {
            transcript.append_root_block(
                NewBlock::new(BlockKind::SystemNote, "branch summary")
                    .with_content(summary.clone()),
            );
            *last_assistant_root = None;
        }
        SessionEntry::Compaction {
            summary,
            tokens_before,
            ..
        } => {
            let content = format!(
                "[compaction: {} tokens summarized]\n\n{}",
                tokens_before, summary
            );
            transcript.append_root_block(
                NewBlock::new(BlockKind::SystemNote, "compaction")
                    .with_content(content)
                    .with_expandable(true)
                    .with_collapsed(true),
            );
            *last_assistant_root = None;
        }
        _ => {}
    }

    Ok(())
}
