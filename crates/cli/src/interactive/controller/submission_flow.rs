use super::*;
use crate::slash::{dispatch_local_slash_command, LocalSlashCommandHost};

impl InteractiveMode {
    /// Drain pending extension UI notifications and display them in the TUI.
    /// Also reads extension statuses and surfaces them in the footer area.
    pub(super) async fn drain_extension_notifications(&mut self) {
        let handler = self
            .session_setup
            .extension_commands
            .get_interactive_ui_handler();
        let Some(handler) = handler else {
            return;
        };
        let notifications = handler.drain_notifications().await;
        let statuses = handler.get_statuses().await;

        for notification in notifications {
            match notification.kind.as_str() {
                "error" => self.show_error(notification.message),
                "warning" => self.show_warning(notification.message),
                _ => self.show_status(notification.message),
            }
        }

        // Surface active statuses in the footer area.
        let active: Vec<String> = statuses
            .values()
            .filter_map(|v| v.as_ref())
            .cloned()
            .collect();
        if !active.is_empty() {
            self.show_status(active.join(" | "));
        }
    }

    pub(super) async fn handle_submitted_text(
        &mut self,
        text: String,
    ) -> InteractiveResult<SubmitOutcome> {
        // If we're waiting for a UI dialog response from an extension,
        // redirect the submit to resolve the dialog.
        if self.streaming.pending_ui_dialog.is_some() {
            let text = text.trim().to_string();
            self.resolve_pending_dialog(Some(text));
            self.clear_editor();
            return Ok(SubmitOutcome::Handled);
        }

        // If we're waiting for auth input (OAuth code paste or API key),
        // redirect the submit to the auth flow.
        if self.streaming.pending_auth_provider.is_some() {
            let key_text = text.trim().to_string();
            // Cancel on empty, bare slash, or explicit /login /logout commands.
            if key_text.is_empty()
                || key_text == "/"
                || key_text == "/login"
                || key_text == "/logout"
            {
                self.cancel_pending_auth();
                self.refresh_ui();
            } else {
                self.finish_auth_login(&key_text);
            }
            self.clear_editor();
            return Ok(SubmitOutcome::Handled);
        }

        let text = text.trim().to_string();
        if text.is_empty() || text == "/" {
            return Ok(SubmitOutcome::Ignored);
        }

        if dispatch_local_slash_command(self, &text)? {
            self.clear_editor();
            return if self.interaction.shutdown_requested {
                Ok(SubmitOutcome::Shutdown)
            } else {
                Ok(SubmitOutcome::Ignored)
            };
        }

        for route in &self.submit_routes {
            let matched = match route.matcher {
                SubmitMatch::Exact(command) => text == command,
                SubmitMatch::Prefix(prefix) => text.starts_with(prefix),
            };
            if !matched {
                continue;
            }

            match route.action {
                SubmitAction::Settings => {
                    self.show_settings_selector();
                    self.clear_editor();
                }
                SubmitAction::ScopedModels => {
                    self.clear_editor();
                    self.show_placeholder("scoped models selector");
                }
                SubmitAction::Model => {
                    let search = text
                        .strip_prefix("/model")
                        .map(str::trim)
                        .filter(|s| !s.is_empty());
                    self.clear_editor();
                    self.handle_model_command(search);
                }
                SubmitAction::Export => {
                    self.handle_export_command(&text);
                    self.clear_editor();
                }
                SubmitAction::Import => {
                    self.handle_import_command(&text);
                    self.clear_editor();
                }
                SubmitAction::Share => {
                    self.show_status("Share is not available.");
                    self.clear_editor();
                }
                SubmitAction::Copy => {
                    self.handle_copy_command();
                    self.clear_editor();
                }
                SubmitAction::Name => {
                    self.handle_name_command(&text);
                    self.clear_editor();
                }
                SubmitAction::Session => {
                    self.handle_session_command();
                    self.clear_editor();
                }
                SubmitAction::Changelog => {
                    self.handle_changelog_command();
                    self.clear_editor();
                }
                SubmitAction::Hotkeys => {
                    self.handle_hotkeys_command();
                    self.clear_editor();
                }
                SubmitAction::Fork => {
                    self.show_user_message_selector();
                    self.clear_editor();
                }
                SubmitAction::Tree => {
                    self.show_tree_selector();
                    self.clear_editor();
                }
                SubmitAction::Login => {
                    self.show_auth_selector(AuthSelectorMode::Login);
                    self.clear_editor();
                }
                SubmitAction::Logout => {
                    self.show_auth_selector(AuthSelectorMode::Logout);
                    self.clear_editor();
                }
                SubmitAction::New => {
                    self.clear_editor();
                    self.handle_new_session();
                }
                SubmitAction::Compact => {
                    let instructions = text
                        .strip_prefix("/compact")
                        .map(str::trim)
                        .filter(|s| !s.is_empty());
                    self.clear_editor();
                    self.handle_compact_command(instructions);
                }
                SubmitAction::Reload => {
                    self.clear_editor();
                    self.handle_reload_command().await?;
                }
                SubmitAction::Debug => {
                    self.handle_debug_command();
                    self.clear_editor();
                }
                SubmitAction::ArminSaysHi => {
                    self.handle_armin_says_hi();
                    self.clear_editor();
                }
                SubmitAction::Resume => {
                    self.show_session_selector();
                    self.clear_editor();
                }
                SubmitAction::Quit => {
                    self.clear_editor();
                    self.shutdown();
                    return Ok(SubmitOutcome::Shutdown);
                }
                SubmitAction::Help => {
                    self.handle_help_command();
                    self.clear_editor();
                }
            }
            return Ok(SubmitOutcome::Ignored);
        }

        if text.starts_with('!') {
            let excluded = text.starts_with("!!");
            let command = if excluded {
                text[2..].trim()
            } else {
                text[1..].trim()
            };
            if !command.is_empty() {
                if self.interaction.is_bash_running {
                    self.show_warning(
                        "A bash command is already running. Press Esc to cancel it first.",
                    );
                    self.set_editor_text(&text);
                    return Ok(SubmitOutcome::Ignored);
                }
                self.push_editor_history(&text);
                self.handle_bash_command(command, excluded);
                self.set_bash_mode(false);
                self.clear_editor();
                return Ok(SubmitOutcome::Ignored);
            }
        }

        if self.interaction.is_compacting {
            if self.is_extension_command(&text) {
                self.push_editor_history(&text);
                self.clear_editor();
                self.render_cache
                    .chat_lines
                    .push(format!("extension> {text}"));
            } else {
                self.queue_compaction_message(text, QueuedMessageKind::Steer);
            }
            return Ok(SubmitOutcome::Ignored);
        }

        if self.streaming.is_streaming {
            self.push_editor_history(&text);
            self.clear_editor();
            self.queues.steering_queue.push_back(text);
            self.sync_pending_render_state();
            return Ok(SubmitOutcome::Ignored);
        }

        self.flush_pending_bash_components();
        if let Some(callback) = self.on_input_callback.as_mut() {
            callback(text.clone());
        }
        self.push_editor_history(&text);
        self.clear_editor();
        self.streaming.pending_working_message = Some(text);
        Ok(SubmitOutcome::Submitted)
    }

    pub(super) async fn dispatch_prompt(&mut self, user_input: String) -> InteractiveResult<()> {
        if let Some(output) = self
            .session_setup
            .extension_commands
            .execute_text(&user_input)
            .await
            .map_err(|err| -> Box<dyn Error + Send + Sync> {
                Box::new(std::io::Error::other(err.to_string()))
            })?
        {
            self.add_chat_message(InteractiveMessage::User {
                text: user_input.clone(),
            });
            if !output.is_empty() {
                self.add_chat_message(InteractiveMessage::Assistant {
                    message: assistant_message_from_parts(&output, None, false),
                    tool_calls: Vec::new(),
                });
            }
            self.drain_extension_notifications().await;
            self.refresh_ui();
            return Ok(());
        }

        let input = self
            .session_setup
            .extension_commands
            .apply_input_hooks(&user_input, "interactive")
            .await
            .map_err(|err| -> Box<dyn Error + Send + Sync> {
                Box::new(std::io::Error::other(err.to_string()))
            })?;
        if input.handled {
            self.add_chat_message(InteractiveMessage::User {
                text: user_input.clone(),
            });
            if let Some(output) = input.output {
                self.add_chat_message(InteractiveMessage::Assistant {
                    message: assistant_message_from_parts(&output, None, false),
                    tool_calls: Vec::new(),
                });
            }
            self.drain_extension_notifications().await;
            self.refresh_ui();
            return Ok(());
        }

        let prompt_text = input.text.unwrap_or(user_input.clone());

        self.controller
            .runtime_host
            .session_mut()
            .prompt(prompt_text.clone(), PromptOptions::default())
            .map_err(|err| -> Box<dyn Error + Send + Sync> { Box::new(err) })?;

        // Show user message IMMEDIATELY with background color (pi-style)
        self.add_chat_message(InteractiveMessage::User {
            text: prompt_text.clone(),
        });
        // Render now so user sees their message before streaming starts.
        // Only redraw after appending the mounted user component.
        self.ui.tui.render();

        // Check if we have credentials before starting.
        if self.session_setup.api_key.trim().is_empty() {
            let provider = self.session_setup.model.provider.clone();
            self.add_chat_message(InteractiveMessage::System {
                text: format!(
                    "No API key for provider '{provider}'. Use /login to authenticate, or /model to switch to an authenticated provider."
                ),
            });
            self.refresh_ui();
            return Ok(());
        }

        // Reset streaming accumulators
        self.streaming.streaming_text.clear();
        self.streaming.streaming_thinking.clear();
        self.streaming.streaming_tool_calls.clear();
        self.streaming.is_streaming = true;

        // Lazily create the session row in the DB on first message.
        if !self.session_setup.session_created {
            let cwd = self.session_setup.tool_ctx.cwd.display().to_string();
            match store::create_session_with_id(
                &self.session_setup.conn,
                &self.session_setup.session_id,
                &cwd,
            ) {
                Ok(_) => self.session_setup.session_created = true,
                Err(e) => {
                    self.show_warning(format!("Failed to create session: {e}"));
                    return Ok(());
                }
            }
        }

        // Append user message to session DB
        {
            let user_entry = bb_core::types::SessionEntry::Message {
                base: bb_core::types::EntryBase {
                    id: bb_core::types::EntryId::generate(),
                    parent_id: self.get_session_leaf(),
                    timestamp: chrono::Utc::now(),
                },
                message: bb_core::types::AgentMessage::User(bb_core::types::UserMessage {
                    content: vec![bb_core::types::ContentBlock::Text {
                        text: prompt_text.clone(),
                    }],
                    timestamp: chrono::Utc::now().timestamp_millis(),
                }),
            };
            store::append_entry(
                &self.session_setup.conn,
                &self.session_setup.session_id,
                &user_entry,
            )
            .map_err(|e| -> Box<dyn Error + Send + Sync> {
                Box::<dyn Error + Send + Sync>::from(e.to_string())
            })?;
        }

        // Auto-name session from first user message if no name is set.
        {
            let session_row =
                store::get_session(&self.session_setup.conn, &self.session_setup.session_id)
                    .ok()
                    .flatten();
            if session_row
                .as_ref()
                .and_then(|r| r.name.as_deref())
                .is_none()
            {
                // Truncate to a reasonable length for display.
                let name = prompt_text.trim().replace('\n', " ");
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
        }

        // Run the streaming turn loop
        self.run_streaming_turn_loop(prompt_text).await?;

        // Ensure all streaming state is fully cleaned up.
        self.streaming.pending_working_message = None;
        self.streaming.status_loader = None;
        self.streaming.is_streaming = false;
        self.clear_status();
        self.invalidate_chat_cache();
        self.rebuild_footer();
        self.refresh_ui();
        Ok(())
    }

    /// Drain steering queue first, then follow-up queue, dispatching each as a new prompt.
    pub(super) async fn drain_queued_messages(&mut self) -> InteractiveResult<()> {
        // First drain all steering messages
        while let Some(text) = self.queues.steering_queue.pop_front() {
            self.sync_pending_render_state();
            self.refresh_ui();
            self.dispatch_prompt(text).await?;
            if self.interaction.shutdown_requested {
                return Ok(());
            }
        }
        // Then drain all follow-up messages
        while let Some(text) = self.queues.follow_up_queue.pop_front() {
            self.sync_pending_render_state();
            self.refresh_ui();
            self.dispatch_prompt(text).await?;
            if self.interaction.shutdown_requested {
                return Ok(());
            }
        }
        self.sync_pending_render_state();
        Ok(())
    }

    /// Poll the dialog receiver for incoming extension UI dialog requests.
    /// If a dialog arrives, store it and show a prompt to the user.
    pub(super) fn poll_pending_dialogs(&mut self) {
        if self.streaming.pending_ui_dialog.is_some() {
            return; // already showing a dialog
        }
        let dialog = match self.streaming.pending_dialog_rx.as_mut() {
            Some(rx) => match rx.try_recv() {
                Ok(d) => d,
                Err(_) => return,
            },
            None => return,
        };

        // Show the dialog prompt to the user
        match &dialog {
            PendingUiDialog::Confirm { title, message, .. } => {
                let label = if title.is_empty() {
                    format!("[extension] {message} (y/n)")
                } else {
                    format!("[extension] {title}: {message} (y/n)")
                };
                self.show_status(label);
            }
            PendingUiDialog::Select { title, options, .. } => {
                let items: Vec<String> = options
                    .iter()
                    .enumerate()
                    .map(|(i, opt)| format!("  {}: {opt}", i + 1))
                    .collect();
                let label = format!(
                    "[extension] {title} (enter number 1-{}):\n{}",
                    options.len(),
                    items.join("\n")
                );
                self.show_status(label);
            }
            PendingUiDialog::Input { prompt, .. } => {
                let label = format!("[extension] {prompt}");
                self.show_status(label);
            }
        }
        self.streaming.pending_ui_dialog = Some(dialog);
        self.refresh_ui();
    }

    /// Resolve the pending UI dialog with the user's input.
    /// For `Confirm`, accepts "y"/"yes" as true, anything else as false.
    /// For `Select`, expects a 1-based number.
    /// For `Input`, forwards the text directly.
    /// Pass `None` to cancel the dialog.
    pub(super) fn resolve_pending_dialog(&mut self, user_input: Option<String>) {
        let Some(dialog) = self.streaming.pending_ui_dialog.take() else {
            return;
        };
        match dialog {
            PendingUiDialog::Confirm { responder, .. } => {
                let confirmed = user_input
                    .as_deref()
                    .map(|s| matches!(s.to_lowercase().as_str(), "y" | "yes"))
                    .unwrap_or(false);
                let _ = responder.send(confirmed);
            }
            PendingUiDialog::Select { responder, options, .. } => {
                let selected = user_input.as_deref().and_then(|s| {
                    let idx: usize = s.trim().parse().ok()?;
                    if idx >= 1 && idx <= options.len() {
                        Some(idx - 1)
                    } else {
                        None
                    }
                });
                let _ = responder.send(selected);
            }
            PendingUiDialog::Input { responder, .. } => {
                let _ = responder.send(user_input);
            }
        }
        self.clear_status();
        self.refresh_ui();
    }

    /// Cancel any pending UI dialog, resolving with defaults.
    pub(super) fn cancel_pending_dialog(&mut self) {
        self.resolve_pending_dialog(None);
    }
}

impl LocalSlashCommandHost for InteractiveMode {
    fn slash_help(&mut self) -> anyhow::Result<()> {
        self.handle_help_command();
        Ok(())
    }

    fn slash_exit(&mut self) -> anyhow::Result<()> {
        self.shutdown();
        Ok(())
    }

    fn slash_new_session(&mut self) -> anyhow::Result<()> {
        self.handle_new_session();
        Ok(())
    }

    fn slash_compact(&mut self, instructions: Option<&str>) -> anyhow::Result<()> {
        self.handle_compact_command(instructions);
        Ok(())
    }

    fn slash_model_select(&mut self, search: Option<&str>) -> anyhow::Result<()> {
        self.handle_model_command(search);
        Ok(())
    }

    fn slash_resume(&mut self) -> anyhow::Result<()> {
        self.show_session_selector();
        Ok(())
    }

    fn slash_tree(&mut self) -> anyhow::Result<()> {
        self.show_tree_selector();
        Ok(())
    }

    fn slash_fork(&mut self) -> anyhow::Result<()> {
        self.show_user_message_selector();
        Ok(())
    }

    fn slash_login(&mut self) -> anyhow::Result<()> {
        self.show_auth_selector(AuthSelectorMode::Login);
        Ok(())
    }

    fn slash_logout(&mut self) -> anyhow::Result<()> {
        self.show_auth_selector(AuthSelectorMode::Logout);
        Ok(())
    }

    fn slash_name(&mut self, name: Option<&str>) -> anyhow::Result<()> {
        match name {
            Some(name) => self.handle_name_command(&format!("/name {name}")),
            None => self.handle_name_command("/name"),
        }
        Ok(())
    }

    fn slash_session_info(&mut self) -> anyhow::Result<()> {
        self.handle_session_command();
        Ok(())
    }

    fn slash_copy(&mut self) -> anyhow::Result<()> {
        self.handle_copy_command();
        Ok(())
    }

    fn slash_settings(&mut self) -> anyhow::Result<()> {
        self.show_settings_selector();
        Ok(())
    }
}
