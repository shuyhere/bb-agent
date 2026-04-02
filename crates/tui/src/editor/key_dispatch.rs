use super::types::{Editor, LastAction};
use crate::select_list::SelectAction;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Simple base64 encoder for OSC 52 clipboard support.
fn base64_encode(input: &str) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes = input.as_bytes();
    let mut result = String::with_capacity((bytes.len() + 2) / 3 * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        result.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
        result.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            result.push(CHARS[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
        if chunk.len() > 2 {
            result.push(CHARS[(triple & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
    }
    result
}

impl Editor {
    pub(super) fn submit(&mut self) -> Option<String> {
        let text = self.get_text().trim().to_string();
        if text.is_empty() {
            return None;
        }
        self.clear();
        self.history_index = -1;
        Some(text)
    }

    pub(super) fn handle_key_event(&mut self, key: &KeyEvent) {
        let KeyEvent {
            code, modifiers, ..
        } = *key;

        if let Some(menu) = &mut self.file_menu {
            match (code, modifiers) {
                (KeyCode::Up, _)
                | (KeyCode::Down, _)
                | (KeyCode::PageUp, _)
                | (KeyCode::PageDown, _)
                | (KeyCode::Home, _)
                | (KeyCode::End, _) => {
                    let _ = menu.handle_key(*key);
                    return;
                }
                (KeyCode::Esc, _) => {
                    self.file_menu = None;
                    return;
                }
                (KeyCode::Enter, KeyModifiers::NONE) | (KeyCode::Tab, KeyModifiers::NONE) => {
                    match menu.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)) {
                        SelectAction::Selected(value) => {
                            self.accept_file_selection(value);
                        }
                        SelectAction::Cancelled | SelectAction::None => {}
                    }
                    return;
                }
                _ => {}
            }
        }

        if let Some(menu) = &mut self.slash_menu {
            match (code, modifiers) {
                (KeyCode::Up, _)
                | (KeyCode::Down, _)
                | (KeyCode::PageUp, _)
                | (KeyCode::PageDown, _)
                | (KeyCode::Home, _)
                | (KeyCode::End, _) => {
                    let _ = menu.handle_key(*key);
                    return;
                }
                (KeyCode::Esc, _) => {
                    self.slash_menu = None;
                    return;
                }
                (KeyCode::Enter, KeyModifiers::NONE) | (KeyCode::Tab, KeyModifiers::NONE) => {
                    match menu.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)) {
                        SelectAction::Selected(value) => {
                            self.accept_slash_selection(value);
                        }
                        SelectAction::Cancelled | SelectAction::None => {}
                    }
                    return;
                }
                _ => {}
            }
        }

        let old_action = std::mem::replace(&mut self.last_action, LastAction::Other);
        // Helper: check if shift is held (for selection extension)
        let shift = modifiers.contains(KeyModifiers::SHIFT);
        let ctrl = modifiers.contains(KeyModifiers::CONTROL);
        let _alt = modifiers.contains(KeyModifiers::ALT);
        let ctrl_shift = ctrl && shift;

        match (code, modifiers) {
            // Submit (Enter, no modifiers)
            (KeyCode::Enter, KeyModifiers::NONE) => {
                // Handled externally via try_submit()
            }

            // Newline (Alt+Enter, Shift+Enter)
            (KeyCode::Enter, KeyModifiers::ALT) | (KeyCode::Enter, KeyModifiers::SHIFT) => {
                self.push_undo();
                self.clear_selection();
                self.new_line();
            }

            // Select all (Ctrl+A)
            (KeyCode::Char('a'), KeyModifiers::CONTROL) => {
                self.select_all();
            }

            // Copy (Ctrl+C)
            (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                if self.has_selection() {
                    if let Some(text) = self.selected_text() {
                        // Try to copy to system clipboard via OSC 52
                        let encoded = base64_encode(&text);
                        let osc = format!("\x1b]52;c;{}\x07", encoded);
                        let _ = std::io::Write::write_all(&mut std::io::stdout(), osc.as_bytes());
                        let _ = std::io::Write::flush(&mut std::io::stdout());
                    }
                }
            }

            // Cut (Ctrl+X)
            (KeyCode::Char('x'), KeyModifiers::CONTROL) => {
                if self.has_selection() {
                    if let Some(text) = self.selected_text() {
                        let encoded = base64_encode(&text);
                        let osc = format!("\x1b]52;c;{}\x07", encoded);
                        let _ = std::io::Write::write_all(&mut std::io::stdout(), osc.as_bytes());
                        let _ = std::io::Write::flush(&mut std::io::stdout());
                    }
                    self.delete_selection();
                }
            }

            // Shift+arrow selection
            (KeyCode::Left, _) if ctrl_shift => {
                self.ensure_anchor();
                self.word_left();
            }
            (KeyCode::Right, _) if ctrl_shift => {
                self.ensure_anchor();
                self.word_right();
            }
            (KeyCode::Left, KeyModifiers::SHIFT) => {
                self.ensure_anchor();
                self.move_left();
            }
            (KeyCode::Right, KeyModifiers::SHIFT) => {
                self.ensure_anchor();
                self.move_right();
            }
            (KeyCode::Up, KeyModifiers::SHIFT) => {
                self.ensure_anchor();
                self.move_up();
            }
            (KeyCode::Down, KeyModifiers::SHIFT) => {
                self.ensure_anchor();
                self.move_down();
            }
            (KeyCode::Home, KeyModifiers::SHIFT) => {
                self.ensure_anchor();
                self.move_to_line_start();
            }
            (KeyCode::End, KeyModifiers::SHIFT) => {
                self.ensure_anchor();
                self.move_to_line_end();
            }

            // Cursor movement (clear selection)
            (KeyCode::Left, KeyModifiers::NONE) => {
                self.clear_selection();
                self.move_left();
            }
            (KeyCode::Right, KeyModifiers::NONE) => {
                self.clear_selection();
                self.move_right();
            }
            (KeyCode::Up, KeyModifiers::NONE) => {
                self.clear_selection();
                if self.is_empty() || self.is_on_first_visual_line() {
                    self.navigate_history(-1);
                } else {
                    self.move_up();
                }
            }
            (KeyCode::Down, KeyModifiers::NONE) => {
                self.clear_selection();
                if self.history_index > -1 && self.is_on_last_visual_line() {
                    self.navigate_history(1);
                } else {
                    self.move_down();
                }
            }

            // Home/End
            (KeyCode::Home, KeyModifiers::NONE) => {
                self.clear_selection();
                self.move_to_line_start();
            }
            (KeyCode::Char('e'), KeyModifiers::CONTROL) | (KeyCode::End, KeyModifiers::NONE) => {
                self.clear_selection();
                self.move_to_line_end();
            }

            // Word jump
            (KeyCode::Left, KeyModifiers::CONTROL) | (KeyCode::Left, KeyModifiers::ALT) => {
                self.clear_selection();
                self.word_left();
            }
            (KeyCode::Right, KeyModifiers::CONTROL) | (KeyCode::Right, KeyModifiers::ALT) => {
                self.clear_selection();
                self.word_right();
            }

            // Deletion
            (KeyCode::Backspace, KeyModifiers::NONE)
            | (KeyCode::Backspace, KeyModifiers::SHIFT) => {
                self.push_undo();
                self.backspace();
            }
            (KeyCode::Delete, _) => {
                self.push_undo();
                self.delete();
            }
            (KeyCode::Char('k'), KeyModifiers::CONTROL) => {
                self.push_undo();
                let accumulate = old_action == LastAction::Kill;
                self.kill_to_end(accumulate);
                self.last_action = LastAction::Kill;
            }
            (KeyCode::Char('u'), KeyModifiers::CONTROL) => {
                self.push_undo();
                let accumulate = old_action == LastAction::Kill;
                self.kill_to_start(accumulate);
                self.last_action = LastAction::Kill;
            }
            (KeyCode::Char('w'), KeyModifiers::CONTROL)
            | (KeyCode::Backspace, KeyModifiers::ALT) => {
                self.push_undo();
                let accumulate = old_action == LastAction::Kill;
                self.delete_word_backward(accumulate);
                self.last_action = LastAction::Kill;
            }

            // Yank
            (KeyCode::Char('y'), KeyModifiers::CONTROL) => {
                self.yank();
            }
            // Yank-pop
            (KeyCode::Char('y'), KeyModifiers::ALT) => {
                self.last_action = old_action;
                self.yank_pop();
            }

            // Undo
            (KeyCode::Char('z'), KeyModifiers::CONTROL) => {
                self.undo();
            }
            // Redo (Ctrl+Shift+Z)
            // Redo (Ctrl+Shift+Z)
            (KeyCode::Char('Z'), KeyModifiers::CONTROL | KeyModifiers::SHIFT) => {
                self.redo();
            }

            // Regular characters
            (KeyCode::Char(c), KeyModifiers::NONE) | (KeyCode::Char(c), KeyModifiers::SHIFT) => {
                self.push_undo();
                self.delete_selection();
                self.insert_char(c);
            }

            // Tab -> 4 spaces
            (KeyCode::Tab, KeyModifiers::NONE) => {
                self.push_undo();
                self.delete_selection();
                self.insert_str("    ");
            }

            _ => {}
        }

        self.update_slash_menu();
        self.update_file_menu();
    }

    pub(super) fn handle_paste_input(&mut self, data: &str) {
        // Handle bracketed paste
        self.push_undo();
        for c in data.chars() {
            if c == '\n' || c == '\r' {
                self.new_line();
            } else if c >= ' ' {
                self.insert_char(c);
            }
        }
        self.last_action = LastAction::Other;
        self.update_slash_menu();
        self.update_file_menu();
    }
}

impl Editor {
    /// Try to take a submitted value. Called from the event loop after Enter.
    pub fn try_submit(&mut self) -> Option<String> {
        if self.slash_menu.is_some() || self.file_menu.is_some() {
            return None;
        }
        self.submit()
    }
}
