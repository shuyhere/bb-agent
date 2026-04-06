use std::collections::HashMap;

use anyhow::Result;
use bb_core::types::{
    AgentMessage, AssistantContent, ContentBlock, EntryBase, EntryId, SessionEntry, StopReason,
    UserMessage,
};
use bb_session::{compaction, context, store, tree};
use bb_tui::fullscreen::{FullscreenCommand, FullscreenNoteLevel, Transcript};
use bb_tui::select_list::SelectItem;
use chrono::Utc;

use super::controller::FullscreenController;
use super::formatting::{format_assistant_text, text_from_blocks};
use super::{FORK_ENTRY_MENU_ID, RESUME_SESSION_MENU_ID, TREE_ENTRY_MENU_ID, TREE_SUMMARY_MENU_ID};

#[cfg(test)]
fn truncate_preview_text(text: &str, max_chars: usize) -> String {
    let total_chars = text.chars().count();
    if total_chars <= max_chars {
        return text.to_string();
    }

    let truncated: String = text.chars().take(max_chars).collect();
    format!("{truncated}…")
}

pub(super) fn build_fullscreen_transcript(
    conn: &rusqlite::Connection,
    session_id: &str,
) -> Result<(
    Transcript,
    HashMap<String, bb_tui::fullscreen::HistoricalToolState>,
)> {
    let session_context = context::build_context(conn, session_id)?;
    let mut transcript = Transcript::new();
    let mut tool_map: HashMap<String, bb_tui::fullscreen::BlockId> = HashMap::new();
    let mut tool_states: HashMap<String, bb_tui::fullscreen::HistoricalToolState> = HashMap::new();
    let mut last_assistant_root: Option<bb_tui::fullscreen::BlockId> = None;

    for message in session_context.messages {
        match message {
            AgentMessage::User(user) => {
                transcript.append_root_block(
                    bb_tui::fullscreen::NewBlock::new(
                        bb_tui::fullscreen::BlockKind::UserMessage,
                        "prompt",
                    )
                    .with_content(text_from_blocks(&user.content, "\n")),
                );
                last_assistant_root = None;
            }
            AgentMessage::Assistant(message) => {
                let content = format_assistant_text(&message);
                let root_id = transcript.append_root_block(
                    bb_tui::fullscreen::NewBlock::new(
                        bb_tui::fullscreen::BlockKind::AssistantMessage,
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
                                bb_tui::fullscreen::NewBlock::new(
                                    bb_tui::fullscreen::BlockKind::Thinking,
                                    "thinking",
                                )
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
                                bb_tui::fullscreen::NewBlock::new(
                                    bb_tui::fullscreen::BlockKind::ToolUse,
                                    bb_tui::fullscreen::format_tool_call_title(name, &raw_args),
                                )
                                .with_content(bb_tui::fullscreen::format_tool_call_content(
                                    name, &raw_args, false,
                                ))
                                .with_expandable(true),
                            )?;
                            tool_map.insert(id.clone(), tool_id);
                            tool_states.insert(
                                id.clone(),
                                bb_tui::fullscreen::HistoricalToolState {
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
                last_assistant_root = Some(root_id);
            }
            AgentMessage::ToolResult(result) => {
                let body = bb_tui::fullscreen::format_tool_result_content(
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
                        bb_tui::fullscreen::NewBlock::new(
                            bb_tui::fullscreen::BlockKind::ToolResult,
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
                } else if let Some(root_id) = last_assistant_root {
                    let tool_use_id = transcript.append_child_block(
                        root_id,
                        bb_tui::fullscreen::NewBlock::new(
                            bb_tui::fullscreen::BlockKind::ToolUse,
                            result.tool_name.clone(),
                        )
                        .with_expandable(true),
                    )?;
                    let _ = transcript.append_child_block(
                        tool_use_id,
                        bb_tui::fullscreen::NewBlock::new(
                            bb_tui::fullscreen::BlockKind::ToolResult,
                            if result.is_error { "error" } else { "output" },
                        )
                        .with_content(body),
                    );
                } else {
                    transcript.append_root_block(
                        bb_tui::fullscreen::NewBlock::new(
                            bb_tui::fullscreen::BlockKind::SystemNote,
                            if result.is_error { "error" } else { "tool" },
                        )
                        .with_content(body),
                    );
                }
            }
            AgentMessage::BashExecution(message) => {
                let tool_id = transcript.append_root_block(
                    bb_tui::fullscreen::NewBlock::new(
                        bb_tui::fullscreen::BlockKind::ToolUse,
                        message.command.clone(),
                    )
                    .with_expandable(true),
                );
                let output = if message.output.is_empty() {
                    String::new()
                } else {
                    message.output
                };
                let _ = transcript.append_child_block(
                    tool_id,
                    bb_tui::fullscreen::NewBlock::new(
                        bb_tui::fullscreen::BlockKind::ToolResult,
                        if message.cancelled {
                            "cancelled"
                        } else {
                            "output"
                        },
                    )
                    .with_content(output),
                );
                last_assistant_root = None;
            }
            AgentMessage::Custom(message) => {
                if message.display {
                    transcript.append_root_block(
                        bb_tui::fullscreen::NewBlock::new(
                            bb_tui::fullscreen::BlockKind::SystemNote,
                            message.custom_type,
                        )
                        .with_content(text_from_blocks(&message.content, "\n")),
                    );
                }
                last_assistant_root = None;
            }
            AgentMessage::BranchSummary(message) => {
                transcript.append_root_block(
                    bb_tui::fullscreen::NewBlock::new(
                        bb_tui::fullscreen::BlockKind::SystemNote,
                        "branch summary",
                    )
                    .with_content(message.summary),
                );
                last_assistant_root = None;
            }
            AgentMessage::CompactionSummary(message) => {
                transcript.append_root_block(
                    bb_tui::fullscreen::NewBlock::new(
                        bb_tui::fullscreen::BlockKind::SystemNote,
                        "compaction",
                    )
                    .with_content(message.summary),
                );
                last_assistant_root = None;
            }
        }
    }

    Ok((transcript, tool_states))
}

impl FullscreenController {
    pub(super) fn ensure_session_row_created(&mut self) -> Result<()> {
        if self.session_setup.session_created {
            return Ok(());
        }

        let cwd = self.session_setup.tool_ctx.cwd.display().to_string();
        store::create_session_with_id(
            &self.session_setup.conn,
            &self.session_setup.session_id,
            &cwd,
        )?;
        self.session_setup.session_created = true;
        Ok(())
    }

    pub(super) fn append_user_entry_to_db_with_images(
        &mut self,
        prompt: &str,
        images: &[super::controller::PendingImage],
    ) -> Result<()> {
        let mut content = vec![ContentBlock::Text {
            text: prompt.to_string(),
        }];
        for img in images {
            content.push(ContentBlock::Image {
                data: img.data.clone(),
                mime_type: img.mime_type.clone(),
            });
        }

        let user_entry = SessionEntry::Message {
            base: EntryBase {
                id: EntryId::generate(),
                parent_id: self.get_session_leaf(),
                timestamp: Utc::now(),
            },
            message: AgentMessage::User(UserMessage {
                content,
                timestamp: Utc::now().timestamp_millis(),
            }),
        };

        store::append_entry(
            &self.session_setup.conn,
            &self.session_setup.session_id,
            &user_entry,
        )?;
        Ok(())
    }

    pub(super) fn auto_name_session(&mut self, prompt: &str) {
        let session_row =
            store::get_session(&self.session_setup.conn, &self.session_setup.session_id)
                .ok()
                .flatten();
        if session_row
            .as_ref()
            .and_then(|row| row.name.as_deref())
            .is_some()
        {
            return;
        }

        let name = prompt.trim().replace('\n', " ");
        let name = if name.chars().count() > 80 {
            let truncated: String = name.chars().take(77).collect();
            format!("{truncated}...")
        } else {
            name
        };

        let _ = store::set_session_name(
            &self.session_setup.conn,
            &self.session_setup.session_id,
            Some(&name),
        );
    }

    pub(super) fn rebuild_current_transcript(&mut self) -> Result<()> {
        let (transcript, tool_states) =
            build_fullscreen_transcript(&self.session_setup.conn, &self.session_setup.session_id)?;
        self.send_command(FullscreenCommand::SetTranscriptWithToolStates {
            transcript,
            tool_states,
        });
        Ok(())
    }

    pub(super) fn handle_new_session(&mut self) {
        let new_id = uuid::Uuid::new_v4().to_string();
        self.options.session_id = Some(new_id.clone());
        self.session_setup.session_id = new_id;
        self.session_setup.session_created = false;
        let _ = self.runtime_host.session_mut().clear_queue();
        self.queued_prompts.clear();
        self.pending_tree_summary_target = None;
        self.pending_tree_custom_prompt_target = None;
        self.pending_images.clear();
        self.retry_status = None;
        if let Some(cancel) = self.local_action_cancel.take() {
            cancel.cancel();
        }
        self.send_command(FullscreenCommand::SetTranscript(Transcript::new()));
        self.send_command(FullscreenCommand::SetInput(String::new()));
        self.publish_footer();
        self.send_command(FullscreenCommand::PushNote {
            level: FullscreenNoteLevel::Status,
            text: "New session started".to_string(),
        });
    }

    pub(super) fn open_resume_menu(&mut self) -> Result<()> {
        let cwd = self.session_setup.tool_ctx.cwd.display().to_string();
        let sessions = store::list_sessions(&self.session_setup.conn, &cwd)?;
        if sessions.is_empty() {
            self.send_command(FullscreenCommand::SetStatusLine(
                "No sessions found in this directory.".to_string(),
            ));
            return Ok(());
        }
        let items = sessions
            .into_iter()
            .map(|row| SelectItem {
                label: row
                    .name
                    .clone()
                    .unwrap_or_else(|| row.session_id.chars().take(8).collect()),
                detail: Some(format!("{} entries • {}", row.entry_count, row.updated_at)),
                value: row.session_id,
            })
            .collect();
        self.send_command(FullscreenCommand::OpenSelectMenu {
            menu_id: RESUME_SESSION_MENU_ID.to_string(),
            title: "Resume session".to_string(),
            items,
            selected_value: None,
        });
        Ok(())
    }

    pub(super) fn handle_resume_session(&mut self, session_id: &str) -> Result<()> {
        self.session_setup.session_id = session_id.to_string();
        self.session_setup.session_created = true;
        self.options.session_id = Some(session_id.to_string());
        let _ = self.runtime_host.session_mut().clear_queue();
        // Clear stale state from previous session's tree interactions.
        self.pending_tree_summary_target = None;
        self.pending_tree_custom_prompt_target = None;
        self.pending_images.clear();
        self.queued_prompts.clear();
        self.streaming = false;
        self.retry_status = None;
        if let Some(cancel) = self.local_action_cancel.take() {
            cancel.cancel();
        }
        self.rebuild_current_transcript()?;
        self.send_command(FullscreenCommand::SetInput(String::new()));
        self.publish_footer();
        self.send_command(FullscreenCommand::SetStatusLine(
            "Resumed session".to_string(),
        ));
        Ok(())
    }

    pub(super) fn open_tree_menu(&mut self, selected_entry_id: Option<&str>) -> Result<()> {
        let tree_nodes = tree::get_tree(&self.session_setup.conn, &self.session_setup.session_id)?;
        if tree_nodes.is_empty() {
            self.send_command(FullscreenCommand::SetStatusLine(
                "No entries in session".to_string(),
            ));
            return Ok(());
        }
        let entries = store::get_entries(&self.session_setup.conn, &self.session_setup.session_id)?;
        let leaf_id = store::get_session(&self.session_setup.conn, &self.session_setup.session_id)?
            .and_then(|row| row.leaf_id);

        self.send_command(FullscreenCommand::OpenTreeMenu {
            menu_id: TREE_ENTRY_MENU_ID.to_string(),
            title: "Session Tree".to_string(),
            tree: tree_nodes,
            entries,
            active_leaf: leaf_id,
            selected_value: selected_entry_id.map(str::to_string),
        });
        Ok(())
    }

    pub(super) fn open_tree_summary_menu(&mut self, entry_id: &str) -> Result<()> {
        self.pending_tree_summary_target = Some(entry_id.to_string());
        self.pending_tree_custom_prompt_target = None;
        self.send_command(FullscreenCommand::OpenSelectMenu {
            menu_id: TREE_SUMMARY_MENU_ID.to_string(),
            title: "Branch summary".to_string(),
            items: vec![
                SelectItem {
                    label: "No summary".to_string(),
                    detail: Some("Jump directly to the selected point".to_string()),
                    value: "none".to_string(),
                },
                SelectItem {
                    label: "Summarize".to_string(),
                    detail: Some("Summarize abandoned branch context".to_string()),
                    value: "summarize".to_string(),
                },
                SelectItem {
                    label: "Summarize with custom prompt".to_string(),
                    detail: Some("Type custom branch-summary instructions".to_string()),
                    value: "custom".to_string(),
                },
            ],
            selected_value: None,
        });
        Ok(())
    }

    pub(super) async fn handle_tree_summary_selection(
        &mut self,
        value: &str,
        submission_rx: &mut tokio::sync::mpsc::UnboundedReceiver<
            bb_tui::fullscreen::FullscreenSubmission,
        >,
    ) -> Result<()> {
        let Some(target_entry_id) = self.pending_tree_summary_target.take() else {
            self.send_command(FullscreenCommand::SetStatusLine(
                "No tree target selected".to_string(),
            ));
            return Ok(());
        };

        match value {
            "none" => self.handle_tree_navigate(&target_entry_id),
            "summarize" => {
                self.summarize_tree_navigation(&target_entry_id, None, false, submission_rx)
                    .await
            }
            "custom" => {
                self.pending_tree_custom_prompt_target = Some(target_entry_id);
                self.send_command(FullscreenCommand::SetInput(String::new()));
                self.send_command(FullscreenCommand::SetStatusLine(
                    "Branch summary instructions (Enter submit, Esc/empty cancels)".to_string(),
                ));
                Ok(())
            }
            _ => {
                self.send_command(FullscreenCommand::SetStatusLine(format!(
                    "Unknown tree summary action: {value}"
                )));
                Ok(())
            }
        }
    }

    pub(super) async fn summarize_tree_navigation(
        &mut self,
        entry_id: &str,
        instructions: Option<&str>,
        replace_instructions: bool,
        submission_rx: &mut tokio::sync::mpsc::UnboundedReceiver<
            bb_tui::fullscreen::FullscreenSubmission,
        >,
    ) -> Result<()> {
        use tokio_util::sync::CancellationToken;

        let cancel = CancellationToken::new();
        self.local_action_cancel = Some(cancel.clone());
        self.send_command(FullscreenCommand::SetStatusLine(
            "Summarizing branch... (Esc to cancel)".to_string(),
        ));

        let current_leaf_id = self.get_session_leaf().map(|id| id.0);
        let summary_mode = match instructions {
            Some(text) => crate::session_navigation::TreeSummaryMode::SummarizeCustom {
                instructions: text.to_string(),
                replace_instructions,
            },
            None => crate::session_navigation::TreeSummaryMode::Summarize,
        };

        enum TreeSummaryAction {
            Cancelled,
            Finished(crate::session_navigation::TreeNavigateOutcome),
            Closed,
        }

        let target_entry_id = entry_id.to_string();
        let action = {
            let navigate = crate::session_navigation::navigate_tree(
                &self.session_setup.conn,
                &self.session_setup.session_id,
                &target_entry_id,
                current_leaf_id.as_deref(),
                summary_mode,
                self.session_setup.provider.as_ref(),
                &self.session_setup.model.id,
                &self.session_setup.api_key,
                &self.session_setup.base_url,
                cancel.clone(),
            );
            tokio::pin!(navigate);

            loop {
                tokio::select! {
                    maybe_submission = submission_rx.recv() => {
                        match maybe_submission {
                            Some(bb_tui::fullscreen::FullscreenSubmission::CancelLocalAction) => {
                                cancel.cancel();
                                break TreeSummaryAction::Cancelled;
                            }
                            Some(bb_tui::fullscreen::FullscreenSubmission::Input(_))
                            | Some(bb_tui::fullscreen::FullscreenSubmission::InputWithImages { .. })
                            | Some(bb_tui::fullscreen::FullscreenSubmission::MenuSelection { .. }) => {}
                            None => {
                                cancel.cancel();
                                break TreeSummaryAction::Closed;
                            }
                        }
                    }
                    outcome = &mut navigate => {
                        break TreeSummaryAction::Finished(outcome?);
                    }
                }
            }
        };
        self.local_action_cancel = None;

        match action {
            TreeSummaryAction::Cancelled => {
                self.send_command(FullscreenCommand::SetInput(String::new()));
                self.send_command(FullscreenCommand::SetStatusLine(
                    "Tree navigation cancelled".to_string(),
                ));
                self.open_tree_summary_menu(&target_entry_id)?;
                Ok(())
            }
            TreeSummaryAction::Finished(outcome) => {
                self.rebuild_current_transcript()?;
                self.publish_footer();
                self.send_command(FullscreenCommand::SetInput(
                    outcome.editor_text.unwrap_or_default(),
                ));
                self.send_command(FullscreenCommand::SetStatusLine(
                    if outcome.summary_entry_id.is_some() {
                        "Summarized branch and navigated".to_string()
                    } else {
                        "Navigated to selected point".to_string()
                    },
                ));
                Ok(())
            }
            TreeSummaryAction::Closed => Ok(()),
        }
    }

    pub(super) fn handle_tree_navigate(&mut self, entry_id: &str) -> Result<()> {
        store::set_leaf(
            &self.session_setup.conn,
            &self.session_setup.session_id,
            Some(entry_id),
        )?;
        self.rebuild_current_transcript()?;
        self.publish_footer();
        self.send_command(FullscreenCommand::SetStatusLine(
            "Navigated to selected point".to_string(),
        ));
        Ok(())
    }

    pub(super) fn open_fork_menu(&mut self) -> Result<()> {
        let rows = store::get_entries(&self.session_setup.conn, &self.session_setup.session_id)?;
        let items: Vec<SelectItem> = rows
            .into_iter()
            .filter_map(|row| {
                let entry = store::parse_entry(&row).ok()?;
                match entry {
                    SessionEntry::Message {
                        base,
                        message: AgentMessage::User(user),
                        ..
                    } => {
                        let text = text_from_blocks(&user.content, " ")
                            .trim()
                            .replace('\n', " ");
                        if text.is_empty() {
                            None
                        } else {
                            Some(SelectItem {
                                label: text.clone(),
                                detail: None,
                                value: base.id.0,
                            })
                        }
                    }
                    _ => None,
                }
            })
            .collect();
        if items.is_empty() {
            self.send_command(FullscreenCommand::SetStatusLine(
                "No messages to fork from".to_string(),
            ));
            return Ok(());
        }
        self.send_command(FullscreenCommand::OpenSelectMenu {
            menu_id: FORK_ENTRY_MENU_ID.to_string(),
            title: "Select a user message to fork from".to_string(),
            items,
            selected_value: None,
        });
        Ok(())
    }

    pub(super) fn handle_fork_from_entry(&mut self, entry_id: &str) -> Result<()> {
        let row = store::get_entry(
            &self.session_setup.conn,
            &self.session_setup.session_id,
            entry_id,
        )?
        .ok_or_else(|| anyhow::anyhow!("Entry not found"))?;
        let entry = store::parse_entry(&row)?;
        let editor_text = match entry {
            SessionEntry::Message {
                message: AgentMessage::User(user),
                ..
            } => text_from_blocks(&user.content, "\n"),
            _ => String::new(),
        };
        store::set_leaf(
            &self.session_setup.conn,
            &self.session_setup.session_id,
            row.parent_id.as_deref(),
        )?;
        self.rebuild_current_transcript()?;
        self.publish_footer();
        self.send_command(FullscreenCommand::SetInput(editor_text));
        self.send_command(FullscreenCommand::SetStatusLine(
            "Forked — edit and send to create a new branch".to_string(),
        ));
        Ok(())
    }

    pub(super) fn handle_compact_command(&mut self, instructions: Option<&str>) -> Result<()> {
        let entries = store::get_entries(&self.session_setup.conn, &self.session_setup.session_id)?;
        let merged_settings =
            bb_core::settings::Settings::load_merged(&self.session_setup.tool_ctx.cwd);
        let settings = bb_core::types::CompactionSettings {
            enabled: merged_settings.compaction.enabled,
            reserve_tokens: merged_settings.compaction.reserve_tokens,
            keep_recent_tokens: merged_settings.compaction.keep_recent_tokens,
        };
        let total_tokens: u64 = entries.iter().map(compaction::estimate_tokens_row).sum();
        let text = match compaction::prepare_compaction(&entries, &settings) {
            Some(prep) => {
                let mut text = format!(
                    "Compaction prepared ({total_tokens} estimated tokens, {} messages to summarize, {} kept)",
                    prep.messages_to_summarize.len(),
                    prep.kept_messages.len()
                );
                if let Some(inst) = instructions.filter(|s| !s.trim().is_empty()) {
                    text.push_str(&format!("\nInstructions: {inst}"));
                }
                text
            }
            None => format!(
                "Nothing to compact ({total_tokens} estimated tokens, {} entries)",
                entries.len()
            ),
        };
        self.send_command(FullscreenCommand::PushNote {
            level: FullscreenNoteLevel::Status,
            text,
        });
        Ok(())
    }

    pub(super) fn get_session_leaf(&self) -> Option<EntryId> {
        crate::turn_runner::get_leaf_raw(&self.session_setup.conn, &self.session_setup.session_id)
    }
}

/// Export session entries to a JSONL file. Returns the absolute path.
pub(super) fn export_session(
    conn: &rusqlite::Connection,
    session_id: &str,
    file_path: &str,
) -> anyhow::Result<String> {
    let rows = store::get_entries(conn, session_id)?;
    let mut lines = Vec::new();
    for row in &rows {
        if let Ok(entry) = store::parse_entry(row)
            && let Ok(json) = serde_json::to_string(&entry)
        {
            lines.push(json);
        }
    }
    std::fs::write(file_path, format!("{}\n", lines.join("\n")))?;
    let abs =
        std::fs::canonicalize(file_path).unwrap_or_else(|_| std::path::PathBuf::from(file_path));
    Ok(abs.display().to_string())
}

#[cfg(test)]
mod tests;
