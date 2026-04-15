use anyhow::Result;
use bb_core::agent_session::PromptOptions;
use bb_tui::tui::{TuiCommand, TuiNoteLevel, TuiSubmission};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::turn_runner::{self, TurnConfig, TurnEvent};

const TURN_RUNNER_JOIN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

use super::controller::{QueuedPrompt, TuiController};

fn is_auto_compaction_status(message: &str) -> bool {
    message.starts_with("Auto-compacted session:")
}

fn is_auto_compaction_terminal_status(message: &str) -> bool {
    is_auto_compaction_status(message) || message.starts_with("Auto-compaction failed:")
}

enum TurnJoinPoll<T> {
    Completed(T),
    TaskFailed(String),
    TimedOut,
}

fn classify_turn_join_poll<T>(
    poll: Result<Result<T, tokio::task::JoinError>, tokio::time::error::Elapsed>,
) -> TurnJoinPoll<T> {
    match poll {
        Ok(Ok(value)) => TurnJoinPoll::Completed(value),
        Ok(Err(err)) => TurnJoinPoll::TaskFailed(err.to_string()),
        Err(_) => TurnJoinPoll::TimedOut,
    }
}

impl TuiController {
    pub(super) async fn dispatch_prompt(
        &mut self,
        prompt: String,
        submission_rx: &mut mpsc::UnboundedReceiver<TuiSubmission>,
    ) -> Result<()> {
        // Drain any pending images into PromptOptions
        let mut opts = PromptOptions::default();
        let pending = std::mem::take(&mut self.pending_images);
        if !pending.is_empty() && !self.session_setup.model.supports_images() {
            self.send_command(TuiCommand::PushNote {
                level: TuiNoteLevel::Warning,
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
            self.send_command(TuiCommand::PushNote {
                level: TuiNoteLevel::Error,
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
        submission_rx: &mut mpsc::UnboundedReceiver<TuiSubmission>,
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
            self.send_command(TuiCommand::PushNote {
                level: TuiNoteLevel::Error,
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
        submission_rx: &mut mpsc::UnboundedReceiver<TuiSubmission>,
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
        let tool_registry = std::mem::take(&mut self.session_setup.tool_registry);

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
            tool_registry,
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
            request_metrics_tracker: self.session_setup.request_metrics_tracker.clone(),
            request_metrics_log_path: self.session_setup.request_metrics_log_path.clone(),
        })
    }

    fn approval_follow_up_prompt(
        choice: bb_tui::tui::TuiApprovalChoice,
        steer_message: Option<&str>,
    ) -> Option<String> {
        if choice == bb_tui::tui::TuiApprovalChoice::Deny {
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

    async fn handle_submission_during_active_turn(
        &mut self,
        maybe_submission: Option<TuiSubmission>,
        submission_rx: &mut mpsc::UnboundedReceiver<TuiSubmission>,
        aborted: &mut bool,
    ) -> Result<bool> {
        match maybe_submission {
            Some(TuiSubmission::ApprovalDecision {
                choice,
                steer_message,
            }) => {
                let follow_up_prompt =
                    Self::approval_follow_up_prompt(choice, steer_message.as_deref());
                self.handle_approval_submission(TuiSubmission::ApprovalDecision {
                    choice,
                    steer_message,
                })?;
                if let Some(prompt) = follow_up_prompt {
                    self.interrupt_turn_with_prompt(prompt);
                    *aborted = true;
                    return Ok(true);
                }
            }
            Some(TuiSubmission::InputWithImages { text, image_paths }) => {
                let has_images = !image_paths.is_empty();
                self.attach_images_from_paths(&image_paths);
                let text = text.trim().to_string();
                if (text.is_empty() && !has_images) || text == "/" {
                    return Ok(false);
                }
                if self.handle_local_submission(&text).await? {
                    if self.shutdown_requested {
                        self.abort_token.cancel();
                        *aborted = true;
                        return Ok(true);
                    }
                    return Ok(false);
                }
                self.queued_prompts.push_back(QueuedPrompt::Visible(text));
                self.publish_status();
                if self.shutdown_requested {
                    self.abort_token.cancel();
                    *aborted = true;
                    return Ok(true);
                }
            }
            Some(TuiSubmission::Input(text)) => {
                let text = text.trim().to_string();
                if text.is_empty() || text == "/" {
                    return Ok(false);
                }
                if self.handle_local_submission(&text).await? {
                    if self.shutdown_requested {
                        self.abort_token.cancel();
                        *aborted = true;
                        return Ok(true);
                    }
                    return Ok(false);
                }
                self.queued_prompts.push_back(QueuedPrompt::Visible(text));
                self.publish_status();
                if self.shutdown_requested {
                    self.abort_token.cancel();
                    *aborted = true;
                    return Ok(true);
                }
            }
            Some(TuiSubmission::MenuSelection { menu_id, value }) => {
                self.handle_menu_selection(&menu_id, &value, submission_rx)
                    .await?;
                if self.shutdown_requested {
                    self.abort_token.cancel();
                    *aborted = true;
                    return Ok(true);
                }
            }
            Some(TuiSubmission::CancelLocalAction) => {
                if self.pending_approval.is_some() {
                    self.handle_approval_submission(TuiSubmission::CancelLocalAction)?;
                } else {
                    // During streaming, cancel aborts the current turn.
                    self.abort_token.cancel();
                    *aborted = true;
                    return Ok(true);
                }
            }
            Some(TuiSubmission::EditQueuedMessages) => {
                if self.queued_prompts.is_empty() {
                    self.send_command(TuiCommand::SetStatusLine(
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
                    self.send_command(TuiCommand::SetInput(queued));
                    self.publish_status();
                }
            }
            None => {
                self.abort_token.cancel();
                *aborted = true;
                return Ok(true);
            }
        }

        Ok(false)
    }

    fn finalize_completed_turn(
        &mut self,
        returned_config: Option<TurnConfig>,
        turn_result: Result<()>,
        aborted: bool,
        saw_context_overflow: bool,
    ) {
        if let Some(config) = returned_config {
            self.session_setup.tool_registry = config.tool_registry;
        }

        if saw_context_overflow {
            self.send_command(TuiCommand::PushNote {
                level: TuiNoteLevel::Warning,
                text: "Context overflow detected after retry. Auto-compaction could not recover this turn; try reducing context or switching to a larger-context model.".to_string(),
            });
        }

        if let Err(err) = turn_result {
            self.send_command(TuiCommand::PushNote {
                level: TuiNoteLevel::Error,
                text: err.to_string(),
            });
        }

        if aborted {
            self.send_command(TuiCommand::TurnAborted);
        }

        self.streaming = false;
        self.retry_status = None;
        self.auto_compaction_in_progress = false;
        self.publish_footer();
        self.publish_status();
    }

    fn drain_pending_turn_events(
        &mut self,
        turn_event_rx: &mut mpsc::UnboundedReceiver<TurnEvent>,
        saw_context_overflow: &mut bool,
    ) {
        while let Ok(event) = turn_event_rx.try_recv() {
            if matches!(&event, TurnEvent::ContextOverflow { .. }) {
                *saw_context_overflow = true;
            }
            self.handle_turn_event(event);
        }
    }

    async fn await_turn_completion(
        &mut self,
        mut turn_handle: tokio::task::JoinHandle<(TurnConfig, Result<()>)>,
        turn_event_rx: &mut mpsc::UnboundedReceiver<TurnEvent>,
        submission_rx: &mut mpsc::UnboundedReceiver<TuiSubmission>,
        aborted: &mut bool,
        saw_context_overflow: &mut bool,
    ) -> Result<(Option<TurnConfig>, Result<()>)> {
        match classify_turn_join_poll(
            tokio::time::timeout(TURN_RUNNER_JOIN_TIMEOUT, &mut turn_handle).await,
        ) {
            TurnJoinPoll::Completed((config, result)) => {
                self.drain_pending_turn_events(turn_event_rx, saw_context_overflow);
                Ok((Some(config), result))
            }
            TurnJoinPoll::TaskFailed(err) => {
                self.send_command(TuiCommand::PushNote {
                    level: TuiNoteLevel::Error,
                    text: format!("Turn runner task failed: {err}"),
                });
                Ok((None, Ok(())))
            }
            TurnJoinPoll::TimedOut => {
                self.send_command(TuiCommand::PushNote {
                    level: TuiNoteLevel::Warning,
                    text: "Timed out waiting for the turn runner to finish; keeping the turn active until it completes".to_string(),
                });

                let mut event_stream_closed = false;
                let mut submission_stream_closed = false;
                let mut approval_stream_closed = false;

                loop {
                    tokio::select! {
                        join_result = &mut turn_handle => {
                            self.drain_pending_turn_events(turn_event_rx, saw_context_overflow);
                            return match join_result {
                                Ok((config, result)) => Ok((Some(config), result)),
                                Err(err) => {
                                    self.send_command(TuiCommand::PushNote {
                                        level: TuiNoteLevel::Error,
                                        text: format!("Turn runner task failed: {err}"),
                                    });
                                    Ok((None, Ok(())))
                                }
                            };
                        }
                        maybe_event = turn_event_rx.recv(), if !event_stream_closed => {
                            match maybe_event {
                                Some(event) => {
                                    if matches!(&event, TurnEvent::ContextOverflow { .. }) {
                                        *saw_context_overflow = true;
                                    }
                                    self.handle_turn_event(event);
                                }
                                None => {
                                    event_stream_closed = true;
                                }
                            }
                        }
                        maybe_approval = self.approval_rx.recv(), if !approval_stream_closed => {
                            match maybe_approval {
                                Some(approval) => self.present_approval_request(approval),
                                None => approval_stream_closed = true,
                            }
                        }
                        maybe_submission = submission_rx.recv(), if !submission_stream_closed => {
                            let submissions_channel_closed = maybe_submission.is_none();
                            let should_break = self
                                .handle_submission_during_active_turn(maybe_submission, submission_rx, aborted)
                                .await?;
                            if submissions_channel_closed {
                                submission_stream_closed = true;
                            }
                            if should_break {
                                continue;
                            }
                        }
                    }
                }
            }
        }
    }

    async fn run_streaming_turn_loop(
        &mut self,
        submission_rx: &mut mpsc::UnboundedReceiver<TuiSubmission>,
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
                    if self
                        .handle_submission_during_active_turn(maybe_prompt, submission_rx, &mut aborted)
                        .await?
                    {
                        break;
                    }
                }
            }
        }

        let (returned_config, turn_result) = self
            .await_turn_completion(
                turn_handle,
                &mut turn_event_rx,
                submission_rx,
                &mut aborted,
                &mut saw_context_overflow,
            )
            .await?;

        self.finalize_completed_turn(returned_config, turn_result, aborted, saw_context_overflow);
        Ok(())
    }

    fn handle_turn_event(&mut self, event: TurnEvent) {
        match event {
            TurnEvent::TurnStart { turn_index } => {
                self.send_command(TuiCommand::TurnStart { turn_index });
            }
            TurnEvent::TextDelta(text) => {
                self.send_command(TuiCommand::TextDelta(text));
            }
            TurnEvent::ThinkingDelta(text) => {
                self.send_command(TuiCommand::ThinkingDelta(text));
            }
            TurnEvent::ToolCallStart { id, name } => {
                self.send_command(TuiCommand::ToolCallStart { id, name });
            }
            TurnEvent::ToolCallDelta { id, args } => {
                self.send_command(TuiCommand::ToolCallDelta { id, args });
            }
            TurnEvent::ToolExecuting { id } => {
                self.send_command(TuiCommand::ToolExecuting { id });
            }
            TurnEvent::ToolOutputDelta { id, chunk } => {
                self.send_command(TuiCommand::ToolOutputDelta { id, chunk });
            }
            TurnEvent::ToolResult {
                id,
                name,
                content,
                details,
                artifact_path,
                is_error,
            } => {
                self.send_command(TuiCommand::ToolResult {
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
                self.send_command(TuiCommand::TurnEnd);
                self.publish_status();
            }
            TurnEvent::ContextOverflow { message } => {
                self.send_command(TuiCommand::PushNote {
                    level: TuiNoteLevel::Warning,
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
                        self.send_command(TuiCommand::PushNote {
                            level: TuiNoteLevel::Error,
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
                    self.send_command(TuiCommand::PushNote {
                        level: TuiNoteLevel::Status,
                        text: message,
                    });
                }
            }
            TurnEvent::Error(message) => {
                self.auto_compaction_in_progress = false;
                self.retry_status = None;
                self.publish_footer();
                self.publish_status();
                self.send_command(TuiCommand::TurnError {
                    message: message.clone(),
                });
                self.send_command(TuiCommand::PushNote {
                    level: TuiNoteLevel::Error,
                    text: message,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        TurnJoinPoll, classify_turn_join_poll, is_auto_compaction_status,
        is_auto_compaction_terminal_status,
    };

    #[tokio::test]
    async fn classifies_join_timeout_without_treating_it_as_completion() {
        let poll = tokio::time::timeout(
            std::time::Duration::from_millis(0),
            std::future::pending::<Result<(), tokio::task::JoinError>>(),
        )
        .await;
        match classify_turn_join_poll(poll) {
            TurnJoinPoll::TimedOut => {}
            _ => panic!("timeout should remain an active-turn wait condition"),
        }
    }

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
