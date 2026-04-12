use std::collections::HashMap;

use anyhow::{Result, anyhow};
use bb_core::agent_session::{ModelRef, ThinkingLevel};
use bb_core::agent_session_runtime::RuntimeModelRef;
use bb_core::settings::Settings;
use bb_core::types::{
    AgentMessage, AssistantContent, ContentBlock, EntryBase, EntryId, SessionEntry, StopReason,
    UserMessage,
};
use bb_provider::Provider;
use bb_provider::anthropic::AnthropicProvider;
use bb_provider::google::GoogleProvider;
use bb_provider::openai::OpenAiProvider;
use bb_provider::registry::{ApiType, ModelRegistry};
use bb_session::{compaction, context, store, tree};
use bb_tui::fullscreen::{BlockKind, FullscreenCommand, FullscreenNoteLevel, NewBlock, Transcript};
use bb_tui::select_list::SelectItem;
use chrono::Utc;

use super::controller::{FullscreenController, ManualCompactionEvent};

const HIDDEN_DISPATCH_PREFIX: &str = "[[bb-hidden-dispatch]]\n";
use super::formatting::{format_assistant_text, format_user_text, text_from_blocks};
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
    let path = tree::active_path(conn, session_id)?;
    let entries: Vec<SessionEntry> = path.iter().map(store::parse_entry).collect::<Result<_>>()?;

    let mut transcript = Transcript::new();
    let mut tool_map: HashMap<String, bb_tui::fullscreen::BlockId> = HashMap::new();
    let mut tool_states: HashMap<String, bb_tui::fullscreen::HistoricalToolState> = HashMap::new();
    let mut last_assistant_root: Option<bb_tui::fullscreen::BlockId> = None;

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
                    append_entry_to_fullscreen_transcript(
                        entry,
                        &mut transcript,
                        &mut tool_map,
                        &mut tool_states,
                        &mut last_assistant_root,
                    )?;
                }
            }

            append_entry_to_fullscreen_transcript(
                &entries[compaction_idx],
                &mut transcript,
                &mut tool_map,
                &mut tool_states,
                &mut last_assistant_root,
            )?;

            for entry in &entries[compaction_idx + 1..] {
                append_entry_to_fullscreen_transcript(
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
            append_entry_to_fullscreen_transcript(
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

fn append_entry_to_fullscreen_transcript(
    entry: &SessionEntry,
    transcript: &mut Transcript,
    tool_map: &mut HashMap<String, bb_tui::fullscreen::BlockId>,
    tool_states: &mut HashMap<String, bb_tui::fullscreen::HistoricalToolState>,
    last_assistant_root: &mut Option<bb_tui::fullscreen::BlockId>,
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
                *last_assistant_root = Some(root_id);
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
                let tool_id = transcript.append_root_block(
                    NewBlock::new(BlockKind::ToolUse, message.command.clone())
                        .with_expandable(true),
                );
                let output = if message.output.is_empty() {
                    String::new()
                } else {
                    message.output.clone()
                };
                let _ = transcript.append_child_block(
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

    pub(super) fn append_hidden_user_entry(&mut self, prompt: &str) -> Result<()> {
        let user_entry = SessionEntry::Message {
            base: EntryBase {
                id: EntryId::generate(),
                parent_id: self.get_session_leaf(),
                timestamp: Utc::now(),
            },
            message: AgentMessage::User(UserMessage {
                content: vec![ContentBlock::Text {
                    text: format!("{HIDDEN_DISPATCH_PREFIX}{prompt}"),
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
        self.manual_compaction_in_progress = false;
        self.manual_compaction_generation += 1;
        if let Some(cancel) = self.local_action_cancel.take() {
            cancel.cancel();
        }
        self.send_command(FullscreenCommand::SetLocalActionActive(false));
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
        self.manual_compaction_in_progress = false;
        self.manual_compaction_generation += 1;
        if let Some(cancel) = self.local_action_cancel.take() {
            cancel.cancel();
        }
        self.send_command(FullscreenCommand::SetLocalActionActive(false));

        if let Ok(session_context) = context::build_context(&self.session_setup.conn, session_id) {
            if let Some(model_info) = session_context.model.clone() {
                let settings = Settings::load_merged(&self.session_setup.tool_ctx.cwd);
                let mut registry = ModelRegistry::new();
                registry.load_custom_models(&settings);
                crate::login::add_cached_github_copilot_models(&mut registry);
                if let Some(model) = registry
                    .find(&model_info.provider, &model_info.model_id)
                    .cloned()
                    .or_else(|| {
                        registry
                            .find_fuzzy(&model_info.model_id, Some(&model_info.provider))
                            .cloned()
                    })
                    .or_else(|| registry.find_fuzzy(&model_info.model_id, None).cloned())
                {
                    let api_key =
                        crate::login::resolve_api_key(&model.provider).unwrap_or_default();
                    let base_url = if model.provider == "github-copilot" {
                        crate::login::github_copilot_api_base_url()
                    } else {
                        model
                            .base_url
                            .clone()
                            .unwrap_or_else(|| "https://api.openai.com/v1".into())
                    };
                    let headers = if model.provider == "github-copilot" {
                        crate::login::github_copilot_runtime_headers()
                    } else {
                        std::collections::HashMap::new()
                    };
                    let provider: std::sync::Arc<dyn Provider> = match model.api {
                        ApiType::AnthropicMessages => std::sync::Arc::new(AnthropicProvider::new()),
                        ApiType::GoogleGenerative => std::sync::Arc::new(GoogleProvider::new()),
                        _ => std::sync::Arc::new(OpenAiProvider::new()),
                    };

                    self.runtime_host.session_mut().set_model(ModelRef {
                        provider: model.provider.clone(),
                        id: model.id.clone(),
                        reasoning: model.reasoning,
                    });
                    self.runtime_host.runtime_mut().model = Some(RuntimeModelRef {
                        provider: model.provider.clone(),
                        id: model.id.clone(),
                        context_window: model.context_window as usize,
                    });
                    self.session_setup.model = model;
                    self.session_setup.provider = provider;
                    self.session_setup.api_key = api_key;
                    self.session_setup.base_url = base_url;
                    self.session_setup.headers = headers.clone();
                    self.session_setup.tool_ctx.web_search = Some(bb_tools::WebSearchRuntime {
                        provider: self.session_setup.provider.clone(),
                        model: self.session_setup.model.clone(),
                        api_key: self.session_setup.api_key.clone(),
                        base_url: self.session_setup.base_url.clone(),
                        headers,
                        enabled: true,
                    });
                    self.options.model_display = Some(format!(
                        "{}/{}",
                        self.session_setup.model.provider, self.session_setup.model.id
                    ));
                }
            }

            let thinking_level = session_context.thinking_level;
            self.session_setup.thinking_level = thinking_level.as_str().to_string();
            self.runtime_host.session_mut().set_thinking_level(
                ThinkingLevel::parse(thinking_level.as_str()).unwrap_or(ThinkingLevel::Medium),
            );
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
                            | Some(bb_tui::fullscreen::FullscreenSubmission::MenuSelection { .. })
                            | Some(bb_tui::fullscreen::FullscreenSubmission::ApprovalDecision { .. })
                            | Some(bb_tui::fullscreen::FullscreenSubmission::EditQueuedMessages) => {}
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

    pub(super) async fn handle_compact_command(
        &mut self,
        instructions: Option<&str>,
    ) -> Result<()> {
        if self.streaming || self.manual_compaction_in_progress {
            self.queued_prompts.push_back(match instructions {
                Some(instructions) => {
                    super::controller::QueuedPrompt::Visible(format!("/compact {instructions}"))
                }
                None => super::controller::QueuedPrompt::Visible("/compact".to_string()),
            });
            self.publish_status();
            return Ok(());
        }

        let merged_settings =
            bb_core::settings::Settings::load_merged(&self.session_setup.tool_ctx.cwd);
        let settings = bb_core::types::CompactionSettings {
            enabled: merged_settings.compaction.enabled,
            reserve_tokens: merged_settings.compaction.reserve_tokens,
            keep_recent_tokens: merged_settings.compaction.keep_recent_tokens,
        };

        use tokio_util::sync::CancellationToken;
        let cancel = CancellationToken::new();
        self.local_action_cancel = Some(cancel.clone());
        self.manual_compaction_in_progress = true;
        self.manual_compaction_generation += 1;
        let generation = self.manual_compaction_generation;
        self.send_command(FullscreenCommand::SetLocalActionActive(true));
        self.send_command(FullscreenCommand::SetStatusLine(
            "Compacting session... (Esc to cancel)".to_string(),
        ));
        self.publish_status();
        self.publish_footer();

        let entries = store::get_entries(&self.session_setup.conn, &self.session_setup.session_id)?;
        let parent_id = crate::turn_runner::get_leaf_raw(
            &self.session_setup.conn,
            &self.session_setup.session_id,
        );
        let db_path = self
            .session_setup
            .conn
            .path()
            .map(std::path::PathBuf::from)
            .ok_or_else(|| anyhow!("Compaction requires a file-backed session database"))?;
        let session_id = self.session_setup.session_id.clone();
        let provider = self.session_setup.provider.clone();
        let model_id = self.session_setup.model.id.clone();
        let api_key = self.session_setup.api_key.clone();
        let base_url = self.session_setup.base_url.clone();
        let headers = self.session_setup.headers.clone();
        let manual_compaction_tx = self.manual_compaction_tx.clone();
        let instructions = instructions.map(str::to_string);

        tokio::spawn(async move {
            let result = crate::compaction_exec::execute_session_compaction(
                entries,
                parent_id,
                db_path,
                &session_id,
                provider,
                &model_id,
                &api_key,
                &base_url,
                &headers,
                &settings,
                instructions.as_deref(),
                cancel,
            )
            .await;
            let _ =
                manual_compaction_tx.send(ManualCompactionEvent::Finished { generation, result });
        });
        Ok(())
    }

    pub(super) async fn handle_manual_compaction_event(
        &mut self,
        event: ManualCompactionEvent,
        submission_rx: &mut tokio::sync::mpsc::UnboundedReceiver<
            bb_tui::fullscreen::FullscreenSubmission,
        >,
    ) -> Result<()> {
        let ManualCompactionEvent::Finished { generation, result } = event;
        if generation != self.manual_compaction_generation {
            return Ok(());
        }

        self.local_action_cancel = None;
        self.manual_compaction_in_progress = false;
        self.send_command(FullscreenCommand::SetLocalActionActive(false));

        match result {
            Ok(result) => {
                self.rebuild_current_transcript()?;
                self.publish_footer();
                self.send_command(FullscreenCommand::SetStatusLine(format!(
                    "Compaction complete • {} messages summarized • {} kept • {} tokens before",
                    result.summarized_count, result.kept_count, result.tokens_before
                )));
            }
            Err(err) if err.to_string() == "Nothing to compact" => {
                let entries =
                    store::get_entries(&self.session_setup.conn, &self.session_setup.session_id)?;
                let total_tokens: u64 = entries.iter().map(compaction::estimate_tokens_row).sum();
                self.publish_footer();
                self.send_command(FullscreenCommand::PushNote {
                    level: FullscreenNoteLevel::Status,
                    text: format!(
                        "Nothing to compact ({total_tokens} estimated tokens, {} entries)",
                        entries.len()
                    ),
                });
                self.send_command(FullscreenCommand::SetStatusLine(
                    "Nothing to compact".to_string(),
                ));
            }
            Err(err) if err.to_string().to_ascii_lowercase().contains("cancel") => {
                self.publish_footer();
                self.send_command(FullscreenCommand::SetStatusLine(
                    "Compaction cancelled".to_string(),
                ));
            }
            Err(err) => {
                self.publish_footer();
                self.send_command(FullscreenCommand::PushNote {
                    level: FullscreenNoteLevel::Error,
                    text: format!("Compaction failed: {err}"),
                });
                self.send_command(FullscreenCommand::SetStatusLine(
                    "Compaction failed".to_string(),
                ));
            }
        }

        if !self.queued_prompts.is_empty() {
            self.drain_queued_prompts(submission_rx).await?;
        }
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
