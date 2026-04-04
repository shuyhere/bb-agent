use std::collections::HashMap;

use anyhow::Result;
use bb_core::types::{
    AgentMessage, AssistantContent, ContentBlock, EntryBase, EntryId, SessionEntry, StopReason,
    UserMessage,
};
use bb_session::{compaction, context, store, tree};
use bb_tui::fullscreen::{
    FullscreenCommand, FullscreenNoteLevel, Transcript,
};
use bb_tui::select_list::SelectItem;
use chrono::Utc;

use super::controller::FullscreenController;
use super::formatting::{
    format_assistant_text, format_tool_arguments, format_tool_result_blocks, text_from_blocks,
    tree_entry_role_and_preview,
};
use super::{FORK_ENTRY_MENU_ID, RESUME_SESSION_MENU_ID, TREE_ENTRY_MENU_ID};

pub(super) fn build_fullscreen_transcript(
    conn: &rusqlite::Connection,
    session_id: &str,
) -> Result<Transcript> {
    let session_context = context::build_context(conn, session_id)?;
    let mut transcript = Transcript::new();
    let mut tool_map: HashMap<String, bb_tui::fullscreen::BlockId> = HashMap::new();
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
                            let tool_id = transcript.append_child_block(
                                root_id,
                                bb_tui::fullscreen::NewBlock::new(
                                    bb_tui::fullscreen::BlockKind::ToolUse,
                                    name.clone(),
                                )
                                .with_content(format_tool_arguments(arguments))
                                .with_expandable(true),
                            )?;
                            tool_map.insert(id.clone(), tool_id);
                        }
                        AssistantContent::Text { .. } => {}
                    }
                }
                last_assistant_root = Some(root_id);
            }
            AgentMessage::ToolResult(result) => {
                let body = format_tool_result_blocks(&result.content);
                if let Some(tool_use_id) = tool_map.get(&result.tool_call_id).copied() {
                    let _ = transcript.append_child_block(
                        tool_use_id,
                        bb_tui::fullscreen::NewBlock::new(
                            bb_tui::fullscreen::BlockKind::ToolResult,
                            if result.is_error { "error" } else { "output" },
                        )
                        .with_content(body),
                    );
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

    Ok(transcript)
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

    pub(super) fn append_user_entry_to_db(&mut self, prompt: &str) -> Result<()> {
        let user_entry = SessionEntry::Message {
            base: EntryBase {
                id: EntryId::generate(),
                parent_id: self.get_session_leaf(),
                timestamp: Utc::now(),
            },
            message: AgentMessage::User(UserMessage {
                content: vec![ContentBlock::Text {
                    text: prompt.to_string(),
                }],
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
        let transcript = build_fullscreen_transcript(
            &self.session_setup.conn,
            &self.session_setup.session_id,
        )?;
        self.send_command(FullscreenCommand::SetTranscript(transcript));
        Ok(())
    }

    pub(super) fn handle_new_session(&mut self) {
        let new_id = uuid::Uuid::new_v4().to_string();
        self.options.session_id = Some(new_id.clone());
        self.session_setup.session_id = new_id;
        self.session_setup.session_created = false;
        let _ = self.runtime_host.session_mut().clear_queue();
        self.queued_prompts.clear();
        self.retry_status = None;
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
        });
        Ok(())
    }

    pub(super) fn handle_resume_session(&mut self, session_id: &str) -> Result<()> {
        self.session_setup.session_id = session_id.to_string();
        self.session_setup.session_created = true;
        self.options.session_id = Some(session_id.to_string());
        let _ = self.runtime_host.session_mut().clear_queue();
        self.rebuild_current_transcript()?;
        self.send_command(FullscreenCommand::SetInput(String::new()));
        self.publish_footer();
        self.send_command(FullscreenCommand::SetStatusLine(
            "Resumed session".to_string(),
        ));
        Ok(())
    }

    pub(super) fn open_tree_menu(&mut self) -> Result<()> {
        let tree_nodes =
            tree::get_tree(&self.session_setup.conn, &self.session_setup.session_id)?;
        if tree_nodes.is_empty() {
            self.send_command(FullscreenCommand::SetStatusLine(
                "No entries in session".to_string(),
            ));
            return Ok(());
        }
        let entries =
            store::get_entries(&self.session_setup.conn, &self.session_setup.session_id)?;
        let mut previews: HashMap<String, (String, String)> = HashMap::new();
        for row in &entries {
            if let Ok(entry) = store::parse_entry(row) {
                previews.insert(row.entry_id.clone(), tree_entry_role_and_preview(&entry));
            }
        }
        let leaf_id =
            store::get_session(&self.session_setup.conn, &self.session_setup.session_id)?
                .and_then(|row| row.leaf_id);

        fn flatten(
            node: &bb_session::tree::TreeNode,
            prefix: &str,
            is_last: bool,
            is_root: bool,
            previews: &HashMap<String, (String, String)>,
            leaf_id: Option<&str>,
            out: &mut Vec<SelectItem>,
        ) {
            let (role, preview) = previews
                .get(&node.entry_id)
                .cloned()
                .unwrap_or_else(|| ("other".to_string(), node.entry_type.clone()));

            // Skip labels and other non-message entries
            let show = matches!(
                role.as_str(),
                "user" | "assistant" | "tool_result" | "compaction" | "branch_summary"
            );

            if show {
                let connector = if is_root {
                    "".to_string()
                } else if is_last {
                    format!("{prefix}\u{2514}\u{2500} ")
                } else {
                    format!("{prefix}\u{251c}\u{2500} ")
                };

                let is_leaf = leaf_id == Some(node.entry_id.as_str());
                let leaf_mark = if is_leaf { "\u{25cf} " } else { "" };

                let role_label = match role.as_str() {
                    "user" => "you",
                    "assistant" => "agent",
                    "tool_result" => "tool",
                    "compaction" => "compact",
                    "branch_summary" => "summary",
                    _ => "other",
                };

                let preview_text = preview.trim().replace('\n', " ");
                let truncated = if preview_text.len() > 55 {
                    format!("{}\u{2026}", &preview_text[..55])
                } else {
                    preview_text
                };

                let branch_info = if node.children.len() > 1 {
                    format!(" \u{2500}\u{252c}\u{2500} {} branches", node.children.len())
                } else {
                    String::new()
                };

                out.push(SelectItem {
                    label: format!(
                        "{connector}{leaf_mark}{role_label}: {truncated}{branch_info}"
                    ),
                    detail: None,
                    value: node.entry_id.clone(),
                });
            }

            let child_prefix = if is_root {
                String::new()
            } else if is_last {
                format!("{prefix}   ")
            } else {
                format!("{prefix}\u{2502}  ")
            };

            let child_count = node.children.len();
            for (i, child) in node.children.iter().enumerate() {
                let child_is_last = i == child_count - 1;
                flatten(
                    child,
                    if show { &child_prefix } else { prefix },
                    child_is_last,
                    false,
                    previews,
                    leaf_id,
                    out,
                );
            }
        }

        let mut items = Vec::new();
        let root_count = tree_nodes.len();
        for (i, node) in tree_nodes.iter().enumerate() {
            flatten(
                node,
                "",
                i == root_count - 1,
                true,
                &previews,
                leaf_id.as_deref(),
                &mut items,
            );
        }
        self.send_command(FullscreenCommand::OpenSelectMenu {
            menu_id: TREE_ENTRY_MENU_ID.to_string(),
            title: "Session Tree".to_string(),
            items,
        });
        Ok(())
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
        let rows =
            store::get_entries(&self.session_setup.conn, &self.session_setup.session_id)?;
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
                        let text =
                            text_from_blocks(&user.content, " ").trim().replace('\n', " ");
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
        let entries =
            store::get_entries(&self.session_setup.conn, &self.session_setup.session_id)?;
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
        if let Ok(entry) = store::parse_entry(row) {
            if let Ok(json) = serde_json::to_string(&entry) {
                lines.push(json);
            }
        }
    }
    std::fs::write(file_path, format!("{}\n", lines.join("\n")))?;
    let abs = std::fs::canonicalize(file_path)
        .unwrap_or_else(|_| std::path::PathBuf::from(file_path));
    Ok(abs.display().to_string())
}
