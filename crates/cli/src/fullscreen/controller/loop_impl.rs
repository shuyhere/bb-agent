use anyhow::Result;
use bb_session::store;
use bb_tui::fullscreen::{
    FullscreenApprovalChoice, FullscreenApprovalDialog, FullscreenCommand, FullscreenNoteLevel,
    FullscreenSubmission,
};
use tokio::sync::mpsc;

use super::{
    FullscreenController, QueuedPrompt, SessionApprovalRule, derive_session_approval_rule,
};

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
                maybe_approval = self.approval_rx.recv() => {
                    let Some(approval) = maybe_approval else {
                        continue;
                    };
                    self.present_approval_request(approval);
                }
                maybe_compaction = self.manual_compaction_rx.recv() => {
                    let Some(event) = maybe_compaction else {
                        continue;
                    };
                    if let Err(err) = self.handle_manual_compaction_event(event, &mut submission_rx).await {
                        self.report_error("manual compaction", &err);
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
        if self.pending_approval.is_some() {
            return self.handle_approval_submission(submission);
        }

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
                // A menu pick may have queued a prompt for the agent (e.g.
                // Shape's "Build agent" menu item dispatches a new turn).
                // Drain the queue so it actually runs.
                if !self.streaming
                    && !self.manual_compaction_in_progress
                    && !self.queued_prompts.is_empty()
                {
                    if let Err(err) = self.drain_queued_prompts(submission_rx).await {
                        self.report_error("drain queued", &err);
                    }
                }
                Ok(())
            }
            FullscreenSubmission::ApprovalDecision { .. } => Ok(()),
            FullscreenSubmission::CancelLocalAction => {
                if let Some(target_entry_id) = self.pending_tree_custom_prompt_target.take() {
                    self.send_command(FullscreenCommand::SetInput(String::new()));
                    self.send_command(FullscreenCommand::SetStatusLine(
                        "Tree navigation cancelled".to_string(),
                    ));
                    self.open_tree_summary_menu(&target_entry_id)?;
                } else if self.pending_login_api_key_provider.take().is_some() {
                    self.send_command(FullscreenCommand::SetLocalActionActive(false));
                    self.send_command(FullscreenCommand::CloseAuthDialog);
                    self.send_command(FullscreenCommand::SetInput(String::new()));
                    self.send_command(FullscreenCommand::SetStatusLine(
                        "Authentication cancelled".to_string(),
                    ));
                } else if self.pending_login_copilot_enterprise {
                    self.pending_login_copilot_enterprise = false;
                    self.send_command(FullscreenCommand::SetLocalActionActive(false));
                    self.send_command(FullscreenCommand::CloseAuthDialog);
                    self.send_command(FullscreenCommand::SetInput(String::new()));
                    self.send_command(FullscreenCommand::SetStatusLine(
                        "Authentication cancelled".to_string(),
                    ));
                } else if let Some(prompt) = self.pending_extension_prompt.take() {
                    self.send_command(FullscreenCommand::SetLocalActionActive(false));
                    self.send_command(FullscreenCommand::CloseAuthDialog);
                    self.send_command(FullscreenCommand::SetInput(String::new()));
                    self.send_command(FullscreenCommand::SetStatusLine(format!(
                        "Cancelled {}",
                        prompt.title
                    )));
                } else if let Some(cancel) = self.local_action_cancel.take() {
                    cancel.cancel();
                } else {
                    self.send_command(FullscreenCommand::SetStatusLine(
                        "press Ctrl+C to exit".to_string(),
                    ));
                }
                Ok(())
            }
            FullscreenSubmission::EditQueuedMessages => {
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
                self.pending_login_api_key_provider = None;
                self.send_command(FullscreenCommand::SetLocalActionActive(false));
                self.send_command(FullscreenCommand::CloseAuthDialog);
                self.send_command(FullscreenCommand::SetStatusLine(
                    "Authentication cancelled".to_string(),
                ));
                return Ok(());
            }
            self.pending_login_api_key_provider = None;
            crate::login::save_api_key(&provider, key)?;
            self.send_command(FullscreenCommand::SetInput(String::new()));
            self.send_command(FullscreenCommand::SetLocalActionActive(false));
            self.send_command(FullscreenCommand::CloseAuthDialog);
            if let Some(display) = self.maybe_switch_to_preferred_post_login_model(&provider) {
                self.send_command(FullscreenCommand::SetStatusLine(format!(
                    "Logged in to {} • switched to {} • use /model to change",
                    crate::login::provider_display_name(&provider),
                    display,
                )));
            } else {
                self.send_command(FullscreenCommand::SetStatusLine(format!(
                    "Logged in to {} • use /model to change",
                    crate::login::provider_display_name(&provider)
                )));
            }
            return Ok(());
        }

        if self.pending_login_copilot_enterprise {
            let domain = text.trim().to_string();
            if domain.is_empty() || domain == "/" {
                self.pending_login_copilot_enterprise = false;
                self.send_command(FullscreenCommand::SetInput(String::new()));
                self.send_command(FullscreenCommand::SetLocalActionActive(false));
                self.send_command(FullscreenCommand::CloseAuthDialog);
                self.send_command(FullscreenCommand::SetStatusLine(
                    "Authentication cancelled".to_string(),
                ));
                return Ok(());
            }
            self.pending_login_copilot_enterprise = false;
            self.finish_copilot_host_setup(&domain)?;
            self.begin_oauth_login("github-copilot", submission_rx)
                .await?;
            return Ok(());
        }

        if let Some(prompt) = self.pending_extension_prompt.clone() {
            let submitted = text.trim().to_string();
            if submitted.is_empty() || submitted == "/" {
                self.pending_extension_prompt = None;
                self.send_command(FullscreenCommand::SetInput(String::new()));
                self.send_command(FullscreenCommand::SetLocalActionActive(false));
                self.send_command(FullscreenCommand::CloseAuthDialog);
                self.send_command(FullscreenCommand::SetStatusLine(format!(
                    "Cancelled {}",
                    prompt.title
                )));
                return Ok(());
            }
            self.pending_extension_prompt = None;
            self.send_command(FullscreenCommand::SetInput(String::new()));
            let invocation = format!(
                "/{} __resume {} -- {}",
                prompt.command, prompt.resume, submitted
            );
            self.execute_extension_command_text(&invocation).await?;
            return Ok(());
        }

        let text = text.trim().to_string();
        if (text.is_empty() && self.pending_images.is_empty()) || text == "/" {
            return Ok(());
        }

        if self.manual_compaction_in_progress {
            self.queued_prompts.push_back(QueuedPrompt::Visible(text));
            self.publish_status();
            return Ok(());
        }

        if self.handle_local_submission(&text).await? {
            if !self.streaming
                && !self.manual_compaction_in_progress
                && !self.queued_prompts.is_empty()
            {
                self.drain_queued_prompts(submission_rx).await?;
            }
            return Ok(());
        }

        let expanded =
            crate::input_files::expand_at_file_references(&text, &self.session_setup.tool_ctx.cwd);
        for warning in expanded.warnings {
            self.send_command(FullscreenCommand::PushNote {
                level: FullscreenNoteLevel::Warning,
                text: warning,
            });
        }
        let image_paths = expanded
            .image_paths
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>();
        if !image_paths.is_empty() {
            self.attach_images_from_paths(&image_paths);
        }
        let prompt_text = expanded.text;

        if self.streaming {
            self.queued_prompts
                .push_back(QueuedPrompt::Visible(prompt_text));
            self.publish_status();
            return Ok(());
        }

        if let Err(err) = self.dispatch_prompt(prompt_text, submission_rx).await {
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

    pub(crate) fn present_approval_request(&mut self, approval: super::PendingApprovalRequest) {
        let request = approval.request.clone();
        if self
            .session_approval_rules
            .iter()
            .any(|rule| rule.matches(&request.command))
        {
            let _ = approval.response_tx.send(bb_tools::ToolApprovalOutcome {
                decision: bb_tools::ToolApprovalDecision::ApprovedForSession,
            });
            self.send_command(FullscreenCommand::SetStatusLine(
                "Approved bash command from session permission".to_string(),
            ));
            return;
        }

        if let Some(pending) = self.pending_approval.replace(approval) {
            let _ = pending.response_tx.send(bb_tools::ToolApprovalOutcome {
                decision: bb_tools::ToolApprovalDecision::Denied,
            });
        }

        let request = self
            .pending_approval
            .as_ref()
            .expect("approval request should exist")
            .request
            .clone();
        let session_rule = derive_session_approval_rule(&request.command);
        self.send_command(FullscreenCommand::SetLocalActionActive(true));
        self.send_command(FullscreenCommand::OpenApprovalDialog(
            FullscreenApprovalDialog {
                title: request.title,
                command: request.command,
                reason: request.reason,
                lines: vec![],
                allow_session: true,
                session_scope_label: Some(session_rule.display_scope()),
                deny_input: String::new(),
                deny_cursor: 0,
                deny_input_placeholder: Some("Tell BB what to do differently".to_string()),
                selected: FullscreenApprovalChoice::ApproveOnce,
            },
        ));
        self.send_command(FullscreenCommand::SetStatusLine(
            "Approval required for bash command".to_string(),
        ));
    }

    pub(crate) fn handle_approval_submission(
        &mut self,
        submission: FullscreenSubmission,
    ) -> Result<()> {
        let (choice, steer_message) = match submission {
            FullscreenSubmission::ApprovalDecision {
                choice,
                steer_message,
            } => (choice, steer_message),
            FullscreenSubmission::CancelLocalAction => (FullscreenApprovalChoice::Deny, None),
            _ => return Ok(()),
        };

        let outcome = match choice {
            FullscreenApprovalChoice::ApproveOnce => bb_tools::ToolApprovalOutcome {
                decision: bb_tools::ToolApprovalDecision::ApprovedOnce,
            },
            FullscreenApprovalChoice::ApproveForSession => {
                if let Some(pending) = self.pending_approval.as_ref() {
                    let rule: SessionApprovalRule =
                        derive_session_approval_rule(&pending.request.command);
                    self.session_approval_rules.insert(rule);
                }
                bb_tools::ToolApprovalOutcome {
                    decision: bb_tools::ToolApprovalDecision::ApprovedForSession,
                }
            }
            FullscreenApprovalChoice::Deny => bb_tools::ToolApprovalOutcome {
                decision: bb_tools::ToolApprovalDecision::Denied,
            },
        };

        if let Some(pending) = self.pending_approval.take() {
            let _ = pending.response_tx.send(outcome);
        }

        self.send_command(FullscreenCommand::CloseApprovalDialog);
        self.send_command(FullscreenCommand::SetLocalActionActive(false));
        self.send_command(FullscreenCommand::SetInput(String::new()));
        self.send_command(FullscreenCommand::SetStatusLine(match choice {
            FullscreenApprovalChoice::ApproveOnce => "Approved bash command".to_string(),
            FullscreenApprovalChoice::ApproveForSession => {
                "Approved bash command for this session".to_string()
            }
            FullscreenApprovalChoice::Deny => {
                if steer_message
                    .as_ref()
                    .is_some_and(|message| !message.trim().is_empty())
                {
                    "Denied bash command with guidance for BB".to_string()
                } else {
                    "Denied bash command".to_string()
                }
            }
        }));
        Ok(())
    }
}
