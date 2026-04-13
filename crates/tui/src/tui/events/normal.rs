use super::*;

impl TuiState {
    pub(super) fn on_normal_key(&mut self, key: KeyEvent) {
        if self.approval_dialog.is_some() {
            self.on_approval_dialog_key(key);
            return;
        }

        if self.auth_dialog.is_some() {
            self.on_auth_dialog_key(key);
            return;
        }

        if let Some(menu) = self.tree_menu.as_mut() {
            let ctrl_submit = matches!(key.code, KeyCode::Char('j') | KeyCode::Char('m'))
                && key.modifiers.contains(KeyModifiers::CONTROL);
            let action = if ctrl_submit {
                menu.selected_value()
                    .map(TreeAction::Selected)
                    .unwrap_or(TreeAction::Cancelled)
            } else {
                menu.handle_key(key)
            };
            match action {
                TreeAction::None => {
                    self.dirty = true;
                }
                TreeAction::Cancelled => {
                    self.tree_menu = None;
                    self.dirty = true;
                }
                TreeAction::Selected(value) => {
                    let menu_id = menu.menu_id.clone();
                    self.tree_menu = None;
                    self.pending_submissions
                        .push_back(TuiSubmission::MenuSelection { menu_id, value });
                    self.dirty = true;
                }
            }
            return;
        }

        if let Some(menu) = self.select_menu.as_mut() {
            let ctrl_submit = matches!(key.code, KeyCode::Char('j') | KeyCode::Char('m'))
                && key.modifiers.contains(KeyModifiers::CONTROL);
            let action = if ctrl_submit {
                menu.list
                    .selected_value()
                    .map(SelectAction::Selected)
                    .unwrap_or(SelectAction::Cancelled)
            } else {
                menu.list.handle_key(key)
            };
            match action {
                SelectAction::None => {
                    self.dirty = true;
                }
                SelectAction::Cancelled => {
                    self.select_menu = None;
                    self.dirty = true;
                }
                SelectAction::Selected(value) => {
                    let menu_id = menu.menu_id.clone();
                    self.select_menu = None;
                    self.pending_submissions
                        .push_back(TuiSubmission::MenuSelection { menu_id, value });
                    self.dirty = true;
                }
            }
            return;
        }

        if let Some(menu) = self.slash_menu.as_mut() {
            let ctrl_submit = matches!(key.code, KeyCode::Char('j') | KeyCode::Char('m'))
                && key.modifiers.contains(KeyModifiers::CONTROL);
            match key.code {
                KeyCode::Tab => {
                    if let Some(value) = menu.selected_value() {
                        self.accept_slash_selection(value);
                    }
                    return;
                }
                KeyCode::Char(' ') if key.modifiers == KeyModifiers::NONE => {
                    if let Some(value) = menu.selected_value() {
                        self.accept_slash_selection(value);
                        self.insert_char(' ');
                    }
                    return;
                }
                _ => {}
            }
            let action = if ctrl_submit {
                menu.list
                    .selected_value()
                    .map(SelectAction::Selected)
                    .unwrap_or(SelectAction::Cancelled)
            } else {
                match key.code {
                    KeyCode::Up
                    | KeyCode::Down
                    | KeyCode::PageUp
                    | KeyCode::PageDown
                    | KeyCode::Home
                    | KeyCode::End
                    | KeyCode::Enter
                    | KeyCode::Esc => menu.list.handle_key(key),
                    _ => SelectAction::None,
                }
            };
            match action {
                SelectAction::None => {}
                SelectAction::Cancelled => {
                    self.slash_menu = None;
                    self.input.clear();
                    self.cursor = 0;
                    self.dirty = true;
                    return;
                }
                SelectAction::Selected(value) => {
                    let exact_match = self.slash_query().as_deref() == Some(value.as_str());
                    if matches!(key.code, KeyCode::Enter) || ctrl_submit {
                        if exact_match {
                            self.submit_local_command(value);
                        } else {
                            self.accept_slash_selection(value);
                        }
                    }
                    return;
                }
            }
            self.dirty = true;
        }

        if let Some(menu) = self.at_file_menu.as_mut() {
            match key.code {
                KeyCode::Tab | KeyCode::Enter => {
                    if let Some(value) = menu.selected_value() {
                        self.accept_at_file_selection(value);
                    } else {
                        self.at_file_menu = None;
                    }
                    self.dirty = true;
                    return;
                }
                KeyCode::Esc => {
                    self.at_file_menu = None;
                    self.dirty = true;
                    return;
                }
                KeyCode::Up | KeyCode::Down => {
                    let action = menu.list.handle_key(key);
                    if let SelectAction::Selected(value) = action {
                        self.accept_at_file_selection(value);
                    }
                    self.dirty = true;
                    return;
                }
                _ => {}
            }
        }

        match key.code {
            KeyCode::Esc => {
                if !self.input.is_empty() {
                    self.input.clear();
                    self.cursor = 0;
                    self.slash_menu = None;
                    self.at_file_menu = None;
                    self.paste_storage.clear();
                    self.paste_counter = 0;
                    self.status_line = String::new();
                } else if !self.viewport.auto_follow {
                    self.viewport.jump_to_bottom();
                    self.status_line = String::new();
                } else if self.has_cancellable_action() {
                    self.pending_submissions
                        .push_back(TuiSubmission::CancelLocalAction);
                    self.status_line = "cancel requested".to_string();
                } else {
                    self.status_line = "press Ctrl+C to exit".to_string();
                }
                self.dirty = true;
            }
            KeyCode::Enter if key.modifiers.contains(KeyModifiers::SHIFT) => {
                self.insert_char('\n');
            }
            KeyCode::Enter => {
                self.submit_input();
            }
            KeyCode::Up if key.modifiers.contains(KeyModifiers::ALT) => {
                self.pending_submissions
                    .push_back(TuiSubmission::EditQueuedMessages);
                self.queued_submission_previews.clear();
                self.editing_queued_messages = true;
                self.status_line = "editing queued messages".to_string();
                self.dirty = true;
            }
            KeyCode::Char('j') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.submit_input();
            }
            KeyCode::Char('m') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.submit_input();
            }
            KeyCode::Backspace => {
                self.backspace();
            }
            KeyCode::Left => {
                self.move_left();
            }
            KeyCode::Right => {
                self.move_right();
            }
            KeyCode::Home => {
                self.cursor = 0;
                self.update_slash_menu();
                self.dirty = true;
            }
            KeyCode::End => {
                self.cursor = self.input.len();
                self.update_slash_menu();
                self.dirty = true;
            }
            KeyCode::Char(ch) if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.status_line = format!("ignored Ctrl+{ch}");
                self.dirty = true;
            }
            KeyCode::Tab => {}
            KeyCode::Char(ch) => {
                self.insert_char(ch);
            }
            _ => {}
        }
    }

    fn on_auth_dialog_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.slash_menu = None;
                self.at_file_menu = None;
                self.pending_submissions
                    .push_back(TuiSubmission::CancelLocalAction);
                self.status_line = "cancel requested".to_string();
                self.dirty = true;
            }
            KeyCode::Enter => {
                self.submit_auth_dialog_input();
            }
            KeyCode::Char('j' | 'm') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.submit_auth_dialog_input();
            }
            KeyCode::Backspace => {
                self.backspace();
                self.slash_menu = None;
                self.at_file_menu = None;
                self.dirty = true;
            }
            KeyCode::Left => {
                self.move_left();
                self.slash_menu = None;
                self.at_file_menu = None;
                self.dirty = true;
            }
            KeyCode::Right => {
                self.move_right();
                self.slash_menu = None;
                self.at_file_menu = None;
                self.dirty = true;
            }
            KeyCode::Home => {
                self.cursor = 0;
                self.slash_menu = None;
                self.at_file_menu = None;
                self.dirty = true;
            }
            KeyCode::End => {
                self.cursor = self.input.len();
                self.slash_menu = None;
                self.at_file_menu = None;
                self.dirty = true;
            }
            KeyCode::Char('c' | 'C') if key.modifiers.contains(KeyModifiers::ALT) => {
                self.copy_auth_dialog_url();
            }
            KeyCode::F(6) => {
                self.copy_auth_dialog_url();
            }
            KeyCode::Char(ch) if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.status_line = format!("ignored Ctrl+{ch}");
                self.dirty = true;
            }
            KeyCode::Tab => {}
            KeyCode::Char(ch) => {
                self.insert_char(ch);
                self.slash_menu = None;
                self.at_file_menu = None;
                self.dirty = true;
            }
            _ => {}
        }
    }

    fn copy_auth_dialog_url(&mut self) {
        if let Some(url) = self
            .auth_dialog
            .as_ref()
            .and_then(|dialog| dialog.url.clone())
        {
            self.pending_clipboard_copy = Some(url);
            self.status_line = "Copied login URL to clipboard".to_string();
        } else {
            self.status_line = "No login URL to copy yet".to_string();
        }
        self.dirty = true;
    }

    fn submit_auth_dialog_input(&mut self) {
        let submitted = self.input.trim_end().to_string();
        if submitted.trim().is_empty() {
            self.status_line = "empty authentication input ignored".to_string();
            self.dirty = true;
            return;
        }

        self.pending_submissions
            .push_back(TuiSubmission::Input(submitted));
        self.input.clear();
        self.cursor = 0;
        self.slash_menu = None;
        self.at_file_menu = None;
        self.paste_storage.clear();
        self.paste_counter = 0;
        self.status_line = "Submitting authentication input...".to_string();
        self.dirty = true;
    }

    fn on_approval_dialog_key(&mut self, key: KeyEvent) {
        use super::super::types::TuiApprovalChoice;

        let deny_selected = self
            .approval_dialog
            .as_ref()
            .is_some_and(|dialog| dialog.selected == TuiApprovalChoice::Deny);

        match key.code {
            KeyCode::Esc => {
                self.submit_approval_decision(TuiApprovalChoice::Deny, None);
            }
            KeyCode::Enter => {
                self.submit_current_approval_decision();
            }
            KeyCode::Char('j' | 'm') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.submit_current_approval_decision();
            }
            KeyCode::Up => {
                self.step_approval_selection(-1);
            }
            KeyCode::Down | KeyCode::Tab => {
                self.step_approval_selection(1);
            }
            KeyCode::BackTab => {
                self.step_approval_selection(-1);
            }
            KeyCode::Left if deny_selected => {
                self.approval_move_left();
            }
            KeyCode::Right if deny_selected => {
                self.approval_move_right();
            }
            KeyCode::Home if deny_selected => {
                self.approval_move_home();
            }
            KeyCode::End if deny_selected => {
                self.approval_move_end();
            }
            KeyCode::Backspace if deny_selected => {
                self.approval_backspace();
            }
            KeyCode::Char('y' | 'Y') if !deny_selected => {
                self.submit_approval_decision(TuiApprovalChoice::ApproveOnce, None);
            }
            KeyCode::Char('a' | 'A') if !deny_selected => {
                if self
                    .approval_dialog
                    .as_ref()
                    .is_some_and(|dialog| dialog.allow_session)
                {
                    self.submit_approval_decision(
                        TuiApprovalChoice::ApproveForSession,
                        None,
                    );
                }
            }
            KeyCode::Char('d' | 'D' | 'n' | 'N') if !deny_selected => {
                self.set_approval_selection(TuiApprovalChoice::Deny);
            }
            KeyCode::Char(ch)
                if deny_selected && !key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                self.approval_insert_char(ch);
            }
            _ => {
                self.dirty = true;
            }
        }
    }

    fn set_approval_selection(&mut self, selected: super::super::types::TuiApprovalChoice) {
        if let Some(dialog) = self.approval_dialog.as_mut() {
            dialog.selected = selected;
        }
        self.dirty = true;
    }

    fn step_approval_selection(&mut self, delta: isize) {
        use super::super::types::TuiApprovalChoice;

        let Some(dialog) = self.approval_dialog.as_ref() else {
            return;
        };
        let mut options = vec![
            TuiApprovalChoice::ApproveOnce,
            TuiApprovalChoice::Deny,
        ];
        if dialog.allow_session {
            options.insert(1, TuiApprovalChoice::ApproveForSession);
        }
        let current_index = options
            .iter()
            .position(|choice| *choice == dialog.selected)
            .unwrap_or(0);
        let next_index = if delta < 0 {
            current_index.saturating_sub(1)
        } else {
            (current_index + 1).min(options.len().saturating_sub(1))
        };
        self.set_approval_selection(options[next_index]);
    }

    fn current_approval_choice(&self) -> super::super::types::TuiApprovalChoice {
        self.approval_dialog
            .as_ref()
            .map(|dialog| dialog.selected)
            .unwrap_or(super::super::types::TuiApprovalChoice::Deny)
    }

    fn current_approval_steer_message(&self) -> Option<String> {
        self.approval_dialog.as_ref().and_then(|dialog| {
            let trimmed = dialog.deny_input.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        })
    }

    fn submit_current_approval_decision(&mut self) {
        let choice = self.current_approval_choice();
        let steer_message = (choice == super::super::types::TuiApprovalChoice::Deny)
            .then(|| self.current_approval_steer_message())
            .flatten();
        self.submit_approval_decision(choice, steer_message);
    }

    fn submit_approval_decision(
        &mut self,
        choice: super::super::types::TuiApprovalChoice,
        steer_message: Option<String>,
    ) {
        self.pending_submissions
            .push_back(TuiSubmission::ApprovalDecision {
                choice,
                steer_message,
            });
        self.status_line = match choice {
            super::super::types::TuiApprovalChoice::ApproveOnce => {
                "Approval submitted".to_string()
            }
            super::super::types::TuiApprovalChoice::ApproveForSession => {
                "Approved for this session".to_string()
            }
            super::super::types::TuiApprovalChoice::Deny => {
                if self
                    .approval_dialog
                    .as_ref()
                    .is_some_and(|dialog| !dialog.deny_input.trim().is_empty())
                {
                    "Denied with guidance for BB".to_string()
                } else {
                    "Approval denied".to_string()
                }
            }
        };
        self.dirty = true;
    }
}
