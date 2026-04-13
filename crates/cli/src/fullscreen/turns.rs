use anyhow::Result;
use bb_core::agent_session::PromptOptions;
use bb_tui::fullscreen::{FullscreenCommand, FullscreenNoteLevel, FullscreenSubmission};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::session_bootstrap::build_tool_defs;
use crate::turn_runner::{self, TurnConfig, TurnEvent};

use super::controller::{FullscreenController, QueuedPrompt};

fn is_auto_compaction_status(message: &str) -> bool {
    message.starts_with("Auto-compacted session:")
}

fn is_auto_compaction_terminal_status(message: &str) -> bool {
    is_auto_compaction_status(message) || message.starts_with("Auto-compaction failed:")
}

impl FullscreenController {
    pub(super) async fn dispatch_prompt(
        &mut self,
        prompt: String,
        submission_rx: &mut mpsc::UnboundedReceiver<FullscreenSubmission>,
    ) -> Result<()> {
        // Drain any pending images into PromptOptions
        let mut opts = PromptOptions::default();
        let pending = std::mem::take(&mut self.pending_images);
        if !pending.is_empty() && !self.session_setup.model.supports_images() {
            self.send_command(FullscreenCommand::PushNote {
                level: FullscreenNoteLevel::Warning,
                text: format!(
                    "Model '{}' does not advertise image input support. Attached images may be ignored. Use /model to switch to an image-capable model.",
                    self.session_setup.model.id
                ),
            });
        }
        for img in &pending {
            opts.images.push(bb_core::agent_session::ImageContent {
                source: img.data.clone(),
                mime_type: Some(img.mime_type.clone()),
            });
        }

        self.runtime_host
            .session_mut()
            .prompt(prompt.clone(), opts)
            .map_err(anyhow::Error::new)?;

        if self.session_setup.api_key.trim().is_empty() {
            self.send_command(FullscreenCommand::PushNote {
                level: FullscreenNoteLevel::Error,
                text: format!(
                    "No credentials configured for provider '{}'. Use /login to sign in. After login, bb will switch to your authenticated default model automatically, and you can use /model to choose another configured model.",
                    self.session_setup.model.provider
                ),
            });
            self.publish_status();
            return Ok(());
        }

        self.ensure_session_row_created()?;
        self.append_user_entry_to_db_with_images(&prompt, &pending)?;
        self.auto_name_session(&prompt);
        self.publish_footer();
        self.publish_status();
        self.run_streaming_turn_loop(submission_rx, prompt).await
    }

    pub(super) async fn dispatch_hidden_prompt(
        &mut self,
        prompt: String,
        submission_rx: &mut mpsc::UnboundedReceiver<FullscreenSubmission>,
    ) -> Result<()> {
        let mut opts = PromptOptions::default();
        let pending = std::mem::take(&mut self.pending_images);
        for img in &pending {
            opts.images.push(bb_core::agent_session::ImageContent {
                source: img.data.clone(),
                mime_type: Some(img.mime_type.clone()),
            });
        }

        self.runtime_host
            .session_mut()
            .prompt(prompt.clone(), opts)
            .map_err(anyhow::Error::new)?;

        if self.session_setup.api_key.trim().is_empty() {
            self.send_command(FullscreenCommand::PushNote {
                level: FullscreenNoteLevel::Error,
                text: format!(
                    "No credentials configured for provider '{}'. Use /login to sign in. After login, bb will switch to your authenticated default model automatically, and you can use /model to choose another configured model.",
                    self.session_setup.model.provider
                ),
            });
            self.publish_status();
            return Ok(());
        }

        self.ensure_session_row_created()?;
        self.append_hidden_user_entry(&prompt)?;
        // Hidden prompts should influence the runtime and stream tool usage,
        // but should not appear as visible user chat messages or rename the
        // session to internal workflow text.
        self.publish_footer();
        self.publish_status();
        self.run_streaming_turn_loop(submission_rx, prompt).await
    }

    pub(super) async fn drain_queued_prompts(
        &mut self,
        submission_rx: &mut mpsc::UnboundedReceiver<FullscreenSubmission>,
    ) -> Result<()> {
        while !self.shutdown_requested {
            let Some(queued) = self.queued_prompts.pop_front() else {
                break;
            };
            let (prompt, visible) = match queued {
                QueuedPrompt::Visible(prompt) => (prompt, true),
                QueuedPrompt::Hidden(prompt) => (prompt, false),
            };
            self.publish_status();
            if self.handle_local_submission(&prompt).await? {
                continue;
            }
            if visible {
                self.dispatch_prompt(prompt, submission_rx).await?;
            } else {
                self.dispatch_hidden_prompt(prompt, submission_rx).await?;
            }
        }
        Ok(())
    }

    fn build_turn_config(&mut self) -> Result<TurnConfig> {
        let sibling_conn = if let Some(conn) = self.session_setup.sibling_conn.clone() {
            conn
        } else {
            let conn = turn_runner::open_sibling_conn(&self.session_setup.conn)?;
            self.session_setup.sibling_conn = Some(conn.clone());
            conn
        };
        let tools = std::mem::take(&mut self.session_setup.tools);
        let tool_defs = build_tool_defs(&tools);

        Ok(TurnConfig {
            conn: sibling_conn,
            session_id: self.session_setup.session_id.clone(),
            system_prompt: self.session_setup.system_prompt.clone(),
            model: self.session_setup.model.clone(),
            provider: self.session_setup.provider.clone(),
            api_key: self.session_setup.api_key.clone(),
            base_url: self.session_setup.base_url.clone(),
            headers: self.session_setup.headers.clone(),
            compaction_settings: bb_core::types::CompactionSettings {
                enabled: self.session_setup.compaction_enabled,
                reserve_tokens: self.session_setup.compaction_reserve_tokens,
                keep_recent_tokens: self.session_setup.compaction_keep_recent_tokens,
            },
            tools,
            tool_defs,
            tool_ctx: bb_tools::ToolContext {
                cwd: self.session_setup.tool_ctx.cwd.clone(),
                artifacts_dir: self.session_setup.tool_ctx.artifacts_dir.clone(),
                execution_policy: self.session_setup.tool_ctx.execution_policy,
                on_output: None,
                web_search: self.session_setup.tool_ctx.web_search.clone(),
                execution_mode: self.session_setup.tool_ctx.execution_mode,
                request_approval: self.session_setup.tool_ctx.request_approval.clone(),
            },
            thinking: if self.session_setup.thinking_level == "off" {
                None
            } else {
                Some(self.session_setup.thinking_level.clone())
            },
            retry_enabled: self.session_setup.retry_enabled,
            retry_max_retries: self.session_setup.retry_max_retries,
            retry_base_delay_ms: self.session_setup.retry_base_delay_ms,
            retry_max_delay_ms: self.session_setup.retry_max_delay_ms,
            cancel: self.abort_token.clone(),
            extensions: self.session_setup.extension_commands.clone(),
        })
    }

    fn approval_follow_up_prompt(
        choice: bb_tui::fullscreen::FullscreenApprovalChoice,
        steer_message: Option<&str>,
    ) -> Option<String> {
        if choice == bb_tui::fullscreen::FullscreenApprovalChoice::Deny {
            steer_message
                .map(str::trim)
                .filter(|message| !message.is_empty())
                .map(|message| message.to_string())
        } else {
            None
        }
    }

    fn interrupt_turn_with_prompt(&mut self, prompt: String) {
        self.queued_prompts.push_back(QueuedPrompt::Visible(prompt));
        self.abort_token.cancel();
    }

    async fn run_streaming_turn_loop(
        &mut self,
        submission_rx: &mut mpsc::UnboundedReceiver<FullscreenSubmission>,
        user_prompt: String,
    ) -> Result<()> {
        self.streaming = true;
        self.retry_status = None;
        self.abort_token = CancellationToken::new();
        self.publish_status();

        let turn_config = self.build_turn_config()?;
        let (turn_event_tx, mut turn_event_rx) = mpsc::unbounded_channel::<TurnEvent>();
        let turn_handle = tokio::spawn(async move {
            turn_runner::run_turn(turn_config, turn_event_tx, user_prompt).await
        });

        let mut aborted = false;
        let mut saw_context_overflow = false;

        loop {
            tokio::select! {
                maybe_event = turn_event_rx.recv() => {
                    let Some(event) = maybe_event else {
                        break;
                    };
                    if matches!(&event, TurnEvent::ContextOverflow { .. }) {
                        saw_context_overflow = true;
                    }
                    self.handle_turn_event(event);
                    if self.shutdown_requested {
                        self.abort_token.cancel();
                        aborted = true;
                        break;
                    }
                    if saw_context_overflow {
                        break;
                    }
                }
                maybe_approval = self.approval_rx.recv() => {
                    if let Some(approval) = maybe_approval {
                        self.present_approval_request(approval);
                    }
                }
                maybe_prompt = submission_rx.recv() => {
                    match maybe_prompt {
                        Some(FullscreenSubmission::ApprovalDecision {
                            choice,
                            steer_message,
                        }) => {
                            let follow_up_prompt = Self::approval_follow_up_prompt(
                                choice,
                                steer_message.as_deref(),
                            );
                            self.handle_approval_submission(FullscreenSubmission::ApprovalDecision {
                                choice,
                                steer_message,
                            })?;
                            if let Some(prompt) = follow_up_prompt {
                                self.interrupt_turn_with_prompt(prompt);
                                aborted = true;
                                break;
                            }
                        }
                        Some(FullscreenSubmission::InputWithImages { text, image_paths }) => {
                            let has_images = !image_paths.is_empty();
                            self.attach_images_from_paths(&image_paths);
                            let text = text.trim().to_string();
                            if (text.is_empty() && !has_images) || text == "/" {
                                continue;
                            }
                            if self.handle_local_submission(&text).await? {
                                if self.shutdown_requested {
                                    self.abort_token.cancel();
                                    aborted = true;
                                    break;
                                }
                                continue;
                            }
                            self.queued_prompts.push_back(QueuedPrompt::Visible(text));
                            self.publish_status();
                            if self.shutdown_requested {
                                self.abort_token.cancel();
                                aborted = true;
                                break;
                            }
                        }
                        Some(FullscreenSubmission::Input(text)) => {
                            let text = text.trim().to_string();
                            if text.is_empty() || text == "/" {
                                continue;
                            }
                            if self.handle_local_submission(&text).await? {
                                if self.shutdown_requested {
                                    self.abort_token.cancel();
                                    aborted = true;
                                    break;
                                }
                                continue;
                            }
                            self.queued_prompts.push_back(QueuedPrompt::Visible(text));
                            self.publish_status();
                            if self.shutdown_requested {
                                self.abort_token.cancel();
                                aborted = true;
                                break;
                            }
                        }
                        Some(FullscreenSubmission::MenuSelection { menu_id, value }) => {
                            self.handle_menu_selection(&menu_id, &value, submission_rx).await?;
                            if self.shutdown_requested {
                                self.abort_token.cancel();
                                aborted = true;
                                break;
                            }
                        }
                        Some(FullscreenSubmission::CancelLocalAction) => {
                            if self.pending_approval.is_some() {
                                self.handle_approval_submission(FullscreenSubmission::CancelLocalAction)?;
                            } else {
                                // During streaming, cancel aborts the current turn.
                                self.abort_token.cancel();
                                aborted = true;
                                break;
                            }
                        }
                        Some(FullscreenSubmission::EditQueuedMessages) => {
                            if self.queued_prompts.is_empty() {
                                self.send_command(FullscreenCommand::SetStatusLine(
                                    "No queued messages to edit".to_string(),
                                ));
                            } else {
                                let queued = self
                                    .queued_prompts
                                    .drain(..)
                                    .map(|queued| match queued {
                                        QueuedPrompt::Visible(text) | QueuedPrompt::Hidden(text) => text,
                                    })
                                    .collect::<Vec<_>>()
                                    .join("\n\n");
                                self.send_command(FullscreenCommand::SetInput(queued));
                                self.publish_status();
                            }
                        }
                        None => {
                            self.abort_token.cancel();
                            aborted = true;
                            break;
                        }
                    }
                }
            }
        }

        let (returned_config, turn_result) =
            match tokio::time::timeout(std::time::Duration::from_secs(5), turn_handle).await {
                Ok(Ok((config, result))) => (Some(config), result),
                Ok(Err(err)) => {
                    self.send_command(FullscreenCommand::PushNote {
                        level: FullscreenNoteLevel::Error,
                        text: format!("Turn runner task failed: {err}"),
                    });
                    (None, Ok(()))
                }
                Err(_) => {
                    self.send_command(FullscreenCommand::PushNote {
                        level: FullscreenNoteLevel::Warning,
                        text: "Timed out waiting for the turn runner to finish".to_string(),
                    });
                    (None, Ok(()))
                }
            };

        if let Some(config) = returned_config {
            self.session_setup.tool_defs = config.tool_defs;
            self.session_setup.tools = config.tools;
        }

        if saw_context_overflow {
            self.send_command(FullscreenCommand::PushNote {
                level: FullscreenNoteLevel::Warning,
                text: "Context overflow detected after retry. Auto-compaction could not recover this turn; try reducing context or switching to a larger-context model.".to_string(),
            });
        }

        if let Err(err) = turn_result {
            self.send_command(FullscreenCommand::PushNote {
                level: FullscreenNoteLevel::Error,
                text: err.to_string(),
            });
        }

        if aborted {
            self.send_command(FullscreenCommand::TurnAborted);
        }

        self.streaming = false;
        self.retry_status = None;
        self.auto_compaction_in_progress = false;
        self.publish_footer();
        self.publish_status();
        Ok(())
    }

    fn handle_turn_event(&mut self, event: TurnEvent) {
        match event {
            TurnEvent::TurnStart { turn_index } => {
                self.send_command(FullscreenCommand::TurnStart { turn_index });
            }
            TurnEvent::TextDelta(text) => {
                self.send_command(FullscreenCommand::TextDelta(text));
            }
            TurnEvent::ThinkingDelta(text) => {
                self.send_command(FullscreenCommand::ThinkingDelta(text));
            }
            TurnEvent::ToolCallStart { id, name } => {
                self.send_command(FullscreenCommand::ToolCallStart { id, name });
            }
            TurnEvent::ToolCallDelta { id, args } => {
                self.send_command(FullscreenCommand::ToolCallDelta { id, args });
            }
            TurnEvent::ToolExecuting { id } => {
                self.send_command(FullscreenCommand::ToolExecuting { id });
            }
            TurnEvent::ToolOutputDelta { id, chunk } => {
                self.send_command(FullscreenCommand::ToolOutputDelta { id, chunk });
            }
            TurnEvent::ToolResult {
                id,
                name,
                content,
                details,
                artifact_path,
                is_error,
            } => {
                self.send_command(FullscreenCommand::ToolResult {
                    id,
                    name,
                    content,
                    details,
                    artifact_path,
                    is_error,
                });
            }
            TurnEvent::TurnEnd => {
                self.retry_status = None;
                self.send_command(FullscreenCommand::TurnEnd);
                self.publish_status();
            }
            TurnEvent::ContextOverflow { message } => {
                self.send_command(FullscreenCommand::PushNote {
                    level: FullscreenNoteLevel::Warning,
                    text: message,
                });
            }
            TurnEvent::AutoRetryStart {
                attempt,
                max_attempts,
                delay_ms,
                error_message,
            } => {
                self.retry_status = Some(format!(
                    "Retrying ({attempt}/{max_attempts}) in {}s... {error_message}",
                    ((delay_ms + 500) / 1000).max(1)
                ));
                self.publish_status();
            }
            TurnEvent::AutoRetryEnd => {
                self.retry_status = None;
                self.publish_status();
            }
            TurnEvent::AutoCompactionStart => {
                self.auto_compaction_in_progress = true;
                self.publish_footer();
                self.publish_status();
            }
            TurnEvent::Done { .. } => {}
            TurnEvent::Status(message) => {
                let is_auto_success = is_auto_compaction_status(&message);
                let is_auto_terminal = is_auto_compaction_terminal_status(&message);
                if is_auto_terminal {
                    self.auto_compaction_in_progress = false;
                }
                if is_auto_success {
                    if let Err(err) = self.rebuild_current_transcript() {
                        self.send_command(FullscreenCommand::PushNote {
                            level: FullscreenNoteLevel::Error,
                            text: format!("Failed to rebuild transcript after compaction: {err}"),
                        });
                    } else {
                        self.publish_footer();
                    }
                } else if is_auto_terminal {
                    self.publish_footer();
                }
                self.publish_status();
                if !is_auto_success {
                    self.send_command(FullscreenCommand::PushNote {
                        level: FullscreenNoteLevel::Status,
                        text: message,
                    });
                }
            }
            TurnEvent::Error(message) => {
                self.auto_compaction_in_progress = false;
                self.retry_status = None;
                self.publish_footer();
                self.publish_status();
                self.send_command(FullscreenCommand::TurnError {
                    message: message.clone(),
                });
                self.send_command(FullscreenCommand::PushNote {
                    level: FullscreenNoteLevel::Error,
                    text: message,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{is_auto_compaction_status, is_auto_compaction_terminal_status};

    #[test]
    fn detects_auto_compaction_status_messages() {
        assert!(is_auto_compaction_status(
            "Auto-compacted session: 10 summarized, 5 kept, 12345 tokens before"
        ));
        assert!(!is_auto_compaction_status("Compacted session manually"));
        assert!(!is_auto_compaction_status("Nothing to compact"));
    }

    #[test]
    fn detects_auto_compaction_terminal_messages() {
        assert!(is_auto_compaction_terminal_status(
            "Auto-compacted session: 10 summarized, 5 kept, 12345 tokens before"
        ));
        assert!(is_auto_compaction_terminal_status(
            "Auto-compaction failed: quota exceeded"
        ));
        assert!(!is_auto_compaction_terminal_status("Compacting session..."));
    }
}
