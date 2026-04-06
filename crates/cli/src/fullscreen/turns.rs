use anyhow::Result;
use bb_core::agent_session::PromptOptions;
use bb_tui::fullscreen::{FullscreenCommand, FullscreenNoteLevel, FullscreenSubmission};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::turn_runner::{self, TurnConfig, TurnEvent};

use super::controller::FullscreenController;

impl FullscreenController {
    pub(super) async fn dispatch_prompt(
        &mut self,
        prompt: String,
        submission_rx: &mut mpsc::UnboundedReceiver<FullscreenSubmission>,
    ) -> Result<()> {
        // Drain any pending images into PromptOptions
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
                    "No API key configured for provider '{}'. Configure credentials and try again.",
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

    pub(super) async fn drain_queued_prompts(
        &mut self,
        submission_rx: &mut mpsc::UnboundedReceiver<FullscreenSubmission>,
    ) -> Result<()> {
        while !self.shutdown_requested {
            let Some(prompt) = self.queued_prompts.pop_front() else {
                break;
            };
            self.publish_status();
            self.dispatch_prompt(prompt, submission_rx).await?;
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

        Ok(TurnConfig {
            conn: sibling_conn,
            session_id: self.session_setup.session_id.clone(),
            system_prompt: self.session_setup.system_prompt.clone(),
            model: self.session_setup.model.clone(),
            provider: self.session_setup.provider.clone(),
            api_key: self.session_setup.api_key.clone(),
            base_url: self.session_setup.base_url.clone(),
            headers: self.session_setup.headers.clone(),
            tools,
            tool_defs: self.session_setup.tool_defs.clone(),
            tool_ctx: bb_tools::ToolContext {
                cwd: self.session_setup.tool_ctx.cwd.clone(),
                artifacts_dir: self.session_setup.tool_ctx.artifacts_dir.clone(),
                on_output: None,
                web_search: self.session_setup.tool_ctx.web_search.clone(),
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
                maybe_prompt = submission_rx.recv() => {
                    match maybe_prompt {
                        Some(FullscreenSubmission::InputWithImages { text, image_paths }) => {
                            self.attach_images_from_paths(&image_paths);
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
                            self.queued_prompts.push_back(text);
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
                            self.queued_prompts.push_back(text);
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
                            // During streaming, cancel aborts the current turn.
                            self.abort_token.cancel();
                            aborted = true;
                            break;
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
            self.session_setup.tools = config.tools;
        }

        if saw_context_overflow {
            self.send_command(FullscreenCommand::PushNote {
                level: FullscreenNoteLevel::Warning,
                text: "Context overflow detected. The shared fullscreen path does not auto-compact yet; switch to the legacy interactive mode to recover.".to_string(),
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
            TurnEvent::Done { .. } => {}
            TurnEvent::Error(message) => {
                self.retry_status = None;
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
