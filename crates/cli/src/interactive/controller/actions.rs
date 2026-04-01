use super::*;

impl InteractiveMode {
    pub(super) async fn handle_key_event(&mut self, key: KeyEvent) -> InteractiveResult<Option<String>> {
        // Match pi: overlays own input while open.
        if self.ui.has_overlay() {
            self.ui.handle_key(&key);
            self.process_overlay_actions();
            self.refresh_ui();
            return Ok(None);
        }

        if let Some(action) = self.lookup_key_action(&key) {
            self.handle_key_action(action).await?;
            self.refresh_ui();
            return Ok(None);
        }

        if key.code == KeyCode::Enter && !key.modifiers.contains(KeyModifiers::SHIFT) {
            let text = self.editor_text();
            let outcome = self.handle_submitted_text(text).await?;
            self.refresh_ui();
            return match outcome {
                SubmitOutcome::Ignored => Ok(None),
                SubmitOutcome::Submitted => Ok(Some(self.take_last_submitted_text())),
                SubmitOutcome::Shutdown => Ok(None),
            };
        }

        self.ui.handle_key(&key);
        self.sync_bash_mode_from_editor();
        self.refresh_ui();
        Ok(None)
    }

    pub(super) fn lookup_key_action(&self, key: &KeyEvent) -> Option<KeyAction> {
        self.key_handlers
            .iter()
            .find(|(binding, _)| binding.code == key.code && binding.modifiers == key.modifiers)
            .map(|(_, action)| *action)
    }

    pub(super) fn setup_key_handlers(&mut self) {
        self.key_handlers.clear();
        self.key_handlers.push((
            KeyBinding {
                code: KeyCode::Esc,
                modifiers: KeyModifiers::NONE,
            },
            KeyAction::Escape,
        ));
        self.key_handlers.push((
            KeyBinding {
                code: KeyCode::Char('c'),
                modifiers: KeyModifiers::CONTROL,
            },
            KeyAction::ClearOrInterrupt,
        ));
        self.key_handlers.push((
            KeyBinding {
                code: KeyCode::Char('d'),
                modifiers: KeyModifiers::CONTROL,
            },
            KeyAction::ExitEmpty,
        ));
        self.key_handlers.push((
            KeyBinding {
                code: KeyCode::Char('z'),
                modifiers: KeyModifiers::CONTROL,
            },
            KeyAction::Suspend,
        ));
        self.key_handlers.push((
            KeyBinding {
                code: KeyCode::F(2),
                modifiers: KeyModifiers::NONE,
            },
            KeyAction::CycleThinking,
        ));
        self.key_handlers.push((
            KeyBinding {
                code: KeyCode::F(3),
                modifiers: KeyModifiers::NONE,
            },
            KeyAction::CycleModelForward,
        ));
        self.key_handlers.push((
            KeyBinding {
                code: KeyCode::F(4),
                modifiers: KeyModifiers::NONE,
            },
            KeyAction::CycleModelBackward,
        ));
        self.key_handlers.push((
            KeyBinding {
                code: KeyCode::F(5),
                modifiers: KeyModifiers::NONE,
            },
            KeyAction::SelectModel,
        ));
        self.key_handlers.push((
            KeyBinding {
                code: KeyCode::F(6),
                modifiers: KeyModifiers::NONE,
            },
            KeyAction::ToggleToolExpansion,
        ));
        self.key_handlers.push((
            KeyBinding {
                code: KeyCode::F(7),
                modifiers: KeyModifiers::NONE,
            },
            KeyAction::ToggleThinkingVisibility,
        ));
        self.key_handlers.push((
            KeyBinding {
                code: KeyCode::F(8),
                modifiers: KeyModifiers::NONE,
            },
            KeyAction::OpenExternalEditor,
        ));
        self.key_handlers.push((
            KeyBinding {
                code: KeyCode::F(9),
                modifiers: KeyModifiers::NONE,
            },
            KeyAction::FollowUp,
        ));
        self.key_handlers.push((
            KeyBinding {
                code: KeyCode::F(10),
                modifiers: KeyModifiers::NONE,
            },
            KeyAction::Dequeue,
        ));
        self.key_handlers.push((
            KeyBinding {
                code: KeyCode::F(11),
                modifiers: KeyModifiers::NONE,
            },
            KeyAction::SessionTree,
        ));
        self.key_handlers.push((
            KeyBinding {
                code: KeyCode::F(12),
                modifiers: KeyModifiers::NONE,
            },
            KeyAction::SessionResume,
        ));
        self.key_handlers.push((
            KeyBinding {
                code: KeyCode::Char('l'),
                modifiers: KeyModifiers::CONTROL,
            },
            KeyAction::SelectModel,
        ));
        self.key_handlers.push((
            KeyBinding {
                code: KeyCode::Char('p'),
                modifiers: KeyModifiers::CONTROL,
            },
            KeyAction::CycleModelForward,
        ));
        self.key_handlers.push((
            KeyBinding {
                code: KeyCode::Char('P'),
                modifiers: KeyModifiers::CONTROL | KeyModifiers::SHIFT,
            },
            KeyAction::CycleModelBackward,
        ));
        self.key_handlers.push((
            KeyBinding {
                code: KeyCode::Char('p'),
                modifiers: KeyModifiers::CONTROL | KeyModifiers::SHIFT,
            },
            KeyAction::CycleModelBackward,
        ));
    }

    pub(super) fn setup_editor_submit_handler(&mut self) {
        self.submit_routes = vec![
            SubmitRoute {
                matcher: SubmitMatch::Exact("/settings"),
                action: SubmitAction::Settings,
            },
            SubmitRoute {
                matcher: SubmitMatch::Exact("/scoped-models"),
                action: SubmitAction::ScopedModels,
            },
            SubmitRoute {
                matcher: SubmitMatch::Exact("/model"),
                action: SubmitAction::Model,
            },
            SubmitRoute {
                matcher: SubmitMatch::Prefix("/model "),
                action: SubmitAction::Model,
            },
            SubmitRoute {
                matcher: SubmitMatch::Prefix("/export"),
                action: SubmitAction::Export,
            },
            SubmitRoute {
                matcher: SubmitMatch::Prefix("/import"),
                action: SubmitAction::Import,
            },
            SubmitRoute {
                matcher: SubmitMatch::Exact("/share"),
                action: SubmitAction::Share,
            },
            SubmitRoute {
                matcher: SubmitMatch::Exact("/copy"),
                action: SubmitAction::Copy,
            },
            SubmitRoute {
                matcher: SubmitMatch::Exact("/name"),
                action: SubmitAction::Name,
            },
            SubmitRoute {
                matcher: SubmitMatch::Prefix("/name "),
                action: SubmitAction::Name,
            },
            SubmitRoute {
                matcher: SubmitMatch::Exact("/session"),
                action: SubmitAction::Session,
            },
            SubmitRoute {
                matcher: SubmitMatch::Exact("/changelog"),
                action: SubmitAction::Changelog,
            },
            SubmitRoute {
                matcher: SubmitMatch::Exact("/hotkeys"),
                action: SubmitAction::Hotkeys,
            },
            SubmitRoute {
                matcher: SubmitMatch::Exact("/fork"),
                action: SubmitAction::Fork,
            },
            SubmitRoute {
                matcher: SubmitMatch::Exact("/tree"),
                action: SubmitAction::Tree,
            },
            SubmitRoute {
                matcher: SubmitMatch::Exact("/login"),
                action: SubmitAction::Login,
            },
            SubmitRoute {
                matcher: SubmitMatch::Exact("/logout"),
                action: SubmitAction::Logout,
            },
            SubmitRoute {
                matcher: SubmitMatch::Exact("/new"),
                action: SubmitAction::New,
            },
            SubmitRoute {
                matcher: SubmitMatch::Exact("/compact"),
                action: SubmitAction::Compact,
            },
            SubmitRoute {
                matcher: SubmitMatch::Prefix("/compact "),
                action: SubmitAction::Compact,
            },
            SubmitRoute {
                matcher: SubmitMatch::Exact("/reload"),
                action: SubmitAction::Reload,
            },
            SubmitRoute {
                matcher: SubmitMatch::Exact("/debug"),
                action: SubmitAction::Debug,
            },
            SubmitRoute {
                matcher: SubmitMatch::Exact("/arminsayshi"),
                action: SubmitAction::ArminSaysHi,
            },
            SubmitRoute {
                matcher: SubmitMatch::Exact("/resume"),
                action: SubmitAction::Resume,
            },
            SubmitRoute {
                matcher: SubmitMatch::Exact("/quit"),
                action: SubmitAction::Quit,
            },
            SubmitRoute {
                matcher: SubmitMatch::Exact("/help"),
                action: SubmitAction::Help,
            },
        ];
    }

    pub(super) async fn handle_key_action(&mut self, action: KeyAction) -> InteractiveResult<()> {
        match action {
            KeyAction::Escape => self.handle_escape(),
            KeyAction::ClearOrInterrupt => self.handle_ctrl_c(),
            KeyAction::ExitEmpty => self.handle_ctrl_d(),
            KeyAction::Suspend => self.handle_ctrl_z(),
            KeyAction::CycleThinking => self.cycle_thinking_level(),
            KeyAction::CycleModelForward => self.cycle_model("forward"),
            KeyAction::CycleModelBackward => self.cycle_model("backward"),
            KeyAction::SelectModel => self.show_model_selector(None),
            KeyAction::ToggleToolExpansion => self.toggle_tool_output_expansion(),
            KeyAction::ToggleThinkingVisibility => self.toggle_thinking_block_visibility(),
            KeyAction::OpenExternalEditor => self.show_placeholder("external editor"),
            KeyAction::FollowUp => self.handle_follow_up(),
            KeyAction::Dequeue => self.handle_dequeue(),
            KeyAction::SessionNew => self.handle_new_session(),
            KeyAction::SessionTree => self.show_tree_selector(),
            KeyAction::SessionFork => self.show_user_message_selector(),
            KeyAction::SessionResume => self.show_session_selector(),
            KeyAction::PasteImage => self.handle_clipboard_image_paste(),
        }
        Ok(())
    }

    pub(super) async fn handle_submitted_text(&mut self, text: String) -> InteractiveResult<SubmitOutcome> {
        let text = text.trim().to_string();
        if text.is_empty() {
            return Ok(SubmitOutcome::Ignored);
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
                    self.handle_share_command();
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
                    self.show_placeholder("oauth login selector");
                    self.clear_editor();
                }
                SubmitAction::Logout => {
                    self.show_placeholder("oauth logout selector");
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
                    self.handle_reload_command();
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
                if self.is_bash_running {
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

        if self.is_compacting {
            if self.is_extension_command(&text) {
                self.push_editor_history(&text);
                self.clear_editor();
                self.chat_lines.push(format!("extension> {text}"));
            } else {
                self.queue_compaction_message(text, QueuedMessageKind::Steer);
            }
            return Ok(SubmitOutcome::Ignored);
        }

        if self.is_streaming {
            self.push_editor_history(&text);
            self.clear_editor();
            self.steering_queue.push_back(text);
            self.sync_pending_render_state();
            return Ok(SubmitOutcome::Ignored);
        }

        self.flush_pending_bash_components();
        if let Some(callback) = self.on_input_callback.as_mut() {
            callback(text.clone());
        }
        self.push_editor_history(&text);
        self.clear_editor();
        self.pending_working_message = Some(text);
        Ok(SubmitOutcome::Submitted)
    }

    pub(super) async fn dispatch_prompt(&mut self, user_input: String) -> InteractiveResult<()> {
        self.controller
            .runtime_host
            .session_mut()
            .prompt(user_input.clone(), PromptOptions::default())
            .map_err(|err| -> Box<dyn Error + Send + Sync> { Box::new(err) })?;

        // Show user message IMMEDIATELY with background color (pi-style)
        self.render_state_mut()
            .add_message_to_chat(InteractiveMessage::User {
                text: user_input.clone(),
            });
        // Render now so user sees their message before streaming starts
        self.refresh_ui();

        // Reset streaming accumulators
        self.streaming_text.clear();
        self.streaming_thinking.clear();
        self.streaming_tool_calls.clear();
        self.is_streaming = true;

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
                        text: user_input.clone(),
                    }],
                    timestamp: chrono::Utc::now().timestamp_millis(),
                }),
            };
            store::append_entry(
                &self.session_setup.conn,
                &self.session_setup.session_id,
                &user_entry,
            ).map_err(|e| -> Box<dyn Error + Send + Sync> { Box::<dyn Error + Send + Sync>::from(e.to_string()) })?;
        }

        // Run the streaming turn loop
        self.run_streaming_turn_loop().await?;

        self.pending_working_message = None;
        self.rebuild_footer();
        self.refresh_ui();
        Ok(())
    }

    /// Drain steering queue first, then follow-up queue, dispatching each as a new prompt.
    pub(super) async fn drain_queued_messages(&mut self) -> InteractiveResult<()> {
        // First drain all steering messages
        while let Some(text) = self.steering_queue.pop_front() {
            self.sync_pending_render_state();
            self.refresh_ui();
            self.dispatch_prompt(text).await?;
            if self.shutdown_requested {
                return Ok(());
            }
        }
        // Then drain all follow-up messages
        while let Some(text) = self.follow_up_queue.pop_front() {
            self.sync_pending_render_state();
            self.refresh_ui();
            self.dispatch_prompt(text).await?;
            if self.shutdown_requested {
                return Ok(());
            }
        }
        self.sync_pending_render_state();
        Ok(())
    }
}
