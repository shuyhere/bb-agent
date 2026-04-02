use super::*;

impl InteractiveMode {
    pub(super) async fn handle_key_event(&mut self, key: KeyEvent) -> InteractiveResult<Option<String>> {
        // Match pi: overlays own input while open.
        if self.ui.tui.has_overlay() {
            self.ui.tui.handle_key(&key);
            self.process_overlay_actions();
            self.refresh_ui();
            return Ok(None);
        }

        // Dedicated login dialog mounted in the editor area owns input while active.
        if self.is_login_dialog_active() {
            self.ui.tui.handle_key(&key);
            self.process_login_dialog_action();
            self.render_editor_frame();
            return Ok(None);
        }

        if let Some(action) = self.lookup_key_action(&key) {
            self.handle_key_action(action).await?;
            // Most key actions only change status/footer, not chat.
            // Use render_editor_frame to avoid full chat rebuild.
            self.render_editor_frame();
            return Ok(None);
        }

        if key.code == KeyCode::Enter && !key.modifiers.contains(KeyModifiers::SHIFT) {
            let text = self.editor_text();
            let outcome = self.handle_submitted_text(text).await?;
            self.render_editor_frame();
            return match outcome {
                SubmitOutcome::Ignored | SubmitOutcome::Handled => Ok(None),
                SubmitOutcome::Submitted => Ok(Some(self.take_last_submitted_text())),
                SubmitOutcome::Shutdown => Ok(None),
            };
        }

        self.ui.tui.handle_key(&key);
        self.sync_bash_mode_from_editor();
        // Lightweight render — only editor/status, skip full chat rebuild.
        self.render_editor_frame();
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
}
