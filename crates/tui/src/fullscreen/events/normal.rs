use super::*;

impl FullscreenState {
    pub(super) fn on_normal_key(&mut self, key: KeyEvent) {
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
                        .push_back(FullscreenSubmission::MenuSelection { menu_id, value });
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
                        .push_back(FullscreenSubmission::MenuSelection { menu_id, value });
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
                        .push_back(FullscreenSubmission::CancelLocalAction);
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
                    .push_back(FullscreenSubmission::CancelLocalAction);
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

    fn submit_auth_dialog_input(&mut self) {
        let submitted = self.input.trim_end().to_string();
        if submitted.trim().is_empty() {
            self.status_line = "empty authentication input ignored".to_string();
            self.dirty = true;
            return;
        }

        self.pending_submissions
            .push_back(FullscreenSubmission::Input(submitted));
        self.input.clear();
        self.cursor = 0;
        self.slash_menu = None;
        self.at_file_menu = None;
        self.paste_storage.clear();
        self.paste_counter = 0;
        self.status_line = "Submitting authentication input...".to_string();
        self.dirty = true;
    }
}
