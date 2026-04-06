use anyhow::Result;
use bb_session::store;
use bb_tui::fullscreen::{FullscreenCommand, FullscreenNoteLevel, FullscreenSubmission};
use tokio::sync::mpsc;

use super::FullscreenController;

impl FullscreenController {
    pub(crate) async fn run(
        mut self,
        mut submission_rx: mpsc::UnboundedReceiver<FullscreenSubmission>,
    ) -> Result<()> {
        let settings = bb_core::settings::Settings::load_global();
        if let Some(theme_name) = settings.color_theme.as_deref()
            && let Some(theme) = bb_tui::fullscreen::spinner::ColorTheme::from_name(theme_name)
        {
            self.color_theme = theme;
            self.send_command(FullscreenCommand::SetColorTheme(theme));
        }

        self.publish_footer();
        let show_startup_resources =
            store::get_session(&self.session_setup.conn, &self.session_setup.session_id)?
                .map(|row| row.entry_count == 0)
                .unwrap_or(true);
        self.rebuild_current_transcript()?;
        if show_startup_resources {
            self.show_startup_resources();
        }
        crate::update_check::spawn_update_check_notice_task(
            self.command_tx.clone(),
            self.session_setup.tool_ctx.cwd.clone(),
        );

        if let Some(initial_message) = self.options.initial_message.take() {
            self.submit_initial_message(initial_message, &mut submission_rx)
                .await;
        }
        for message in std::mem::take(&mut self.options.initial_messages) {
            self.submit_initial_message(message, &mut submission_rx)
                .await;
        }

        let mut resource_watch_tick = tokio::time::interval(std::time::Duration::from_secs(1));
        resource_watch_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        while !self.shutdown_requested {
            tokio::select! {
                maybe_submission = submission_rx.recv() => {
                    let Some(submission) = maybe_submission else {
                        self.abort_token.cancel();
                        break;
                    };
                    if let Err(err) = self.handle_submission(submission, &mut submission_rx).await {
                        self.report_error("submission", &err);
                    }
                }
                _ = resource_watch_tick.tick() => {
                    if let Err(err) = self.maybe_auto_reload_resources().await {
                        self.report_error("auto reload", &err);
                    }
                }
            }
        }

        Ok(())
    }

    async fn submit_initial_message(
        &mut self,
        message: String,
        submission_rx: &mut mpsc::UnboundedReceiver<FullscreenSubmission>,
    ) {
        if let Err(err) = self.handle_submitted_text(message, submission_rx).await {
            self.report_error("initial message", &err);
        }
    }

    fn report_error(&mut self, context: &str, err: &anyhow::Error) {
        tracing::error!("{context} error: {err}");
        self.send_command(FullscreenCommand::PushNote {
            level: FullscreenNoteLevel::Error,
            text: format!("Error: {err}"),
        });
    }

    async fn handle_submission(
        &mut self,
        submission: FullscreenSubmission,
        submission_rx: &mut mpsc::UnboundedReceiver<FullscreenSubmission>,
    ) -> Result<()> {
        match submission {
            FullscreenSubmission::Input(text) => {
                self.handle_submitted_text(text, submission_rx).await
            }
            FullscreenSubmission::InputWithImages { text, image_paths } => {
                self.attach_images_from_paths(&image_paths);
                self.handle_submitted_text(text, submission_rx).await
            }
            FullscreenSubmission::MenuSelection { menu_id, value } => {
                if let Err(err) = self
                    .handle_menu_selection(&menu_id, &value, submission_rx)
                    .await
                {
                    self.report_error("menu selection", &err);
                }
                Ok(())
            }
            FullscreenSubmission::CancelLocalAction => {
                if let Some(target_entry_id) = self.pending_tree_custom_prompt_target.take() {
                    self.send_command(FullscreenCommand::SetInput(String::new()));
                    self.send_command(FullscreenCommand::SetStatusLine(
                        "Tree navigation cancelled".to_string(),
                    ));
                    self.open_tree_summary_menu(&target_entry_id)?;
                } else if let Some(cancel) = self.local_action_cancel.take() {
                    cancel.cancel();
                } else {
                    self.send_command(FullscreenCommand::SetStatusLine(
                        "press Ctrl+C to exit".to_string(),
                    ));
                }
                Ok(())
            }
        }
    }

    pub(crate) async fn handle_submitted_text(
        &mut self,
        text: String,
        submission_rx: &mut mpsc::UnboundedReceiver<FullscreenSubmission>,
    ) -> Result<()> {
        if let Some(target_entry_id) = self.pending_tree_custom_prompt_target.clone() {
            let instructions = text.trim().to_string();
            self.pending_tree_custom_prompt_target = None;
            if instructions.is_empty() || instructions == "/" {
                self.send_command(FullscreenCommand::SetInput(String::new()));
                self.send_command(FullscreenCommand::SetStatusLine(
                    "Tree navigation cancelled".to_string(),
                ));
                self.open_tree_summary_menu(&target_entry_id)?;
                return Ok(());
            }

            self.send_command(FullscreenCommand::SetInput(String::new()));
            self.summarize_tree_navigation(
                &target_entry_id,
                Some(&instructions),
                true,
                submission_rx,
            )
            .await?;
            return Ok(());
        }

        if let Some(provider) = self.pending_login_api_key_provider.clone() {
            let key = text.trim().to_string();
            if key.is_empty() || key == "/" {
                self.send_command(FullscreenCommand::SetStatusLine(
                    "API key entry cancelled".to_string(),
                ));
                return Ok(());
            }
            self.pending_login_api_key_provider = None;
            crate::login::save_api_key(&provider, key)?;
            self.send_command(FullscreenCommand::SetInput(String::new()));
            self.send_command(FullscreenCommand::SetStatusLine(format!(
                "Logged in to {}",
                crate::login::provider_display_name(&provider)
            )));
            return Ok(());
        }

        let text = text.trim().to_string();
        if text.is_empty() || text == "/" {
            return Ok(());
        }

        if self.handle_local_submission(&text).await? {
            return Ok(());
        }

        if self.streaming {
            self.queued_prompts.push_back(text);
            self.publish_status();
            return Ok(());
        }

        if let Err(err) = self.dispatch_prompt(text, submission_rx).await {
            self.report_error("prompt dispatch", &err);
            self.streaming = false;
            self.publish_status();
            return Ok(());
        }
        if let Err(err) = self.drain_queued_prompts(submission_rx).await {
            self.report_error("drain queued", &err);
            self.streaming = false;
            self.publish_status();
        }
        Ok(())
    }
}
