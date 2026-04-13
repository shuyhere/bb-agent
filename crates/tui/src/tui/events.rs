mod clipboard;
mod mouse;
mod normal;
mod transcript;

use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};

use crate::select_list::SelectAction;
use crate::tree_selector::TreeAction;

use super::{
    layout::Size,
    projection::ProjectedRowKind,
    runtime::TuiState,
    transcript::{BlockId, BlockKind},
    types::{TuiMode, TuiSubmission},
};
pub(super) use clipboard::cleanup_managed_clipboard_temp_image;
use clipboard::{try_read_clipboard_image, try_read_clipboard_text};

fn is_clipboard_paste_shortcut(key: &KeyEvent) -> bool {
    match key.code {
        KeyCode::Char('v' | 'V') => {
            key.modifiers.contains(KeyModifiers::CONTROL)
                || key.modifiers.contains(KeyModifiers::SUPER)
        }
        KeyCode::Insert => key.modifiers.contains(KeyModifiers::SHIFT),
        _ => false,
    }
}

impl TuiState {
    pub fn on_tick(&mut self) {
        self.tick_count = self.tick_count.wrapping_add(1);
        if self.has_active_turn() || self.has_running_tool() {
            self.spinner.tick();
            self.refresh_running_tool_visuals();
            self.dirty = true;
        } else if self.local_action_active {
            self.spinner.tick();
            self.dirty = true;
        } else if self.should_animate_status() {
            self.dirty = true;
        }
    }

    pub fn on_resize(&mut self, width: u16, height: u16) {
        self.size = Size { width, height };
        if let Some(menu) = self.tree_menu.as_mut() {
            let max_visible = height.saturating_sub(if height >= 8 { 8 } else { 3 }) as usize;
            menu.set_max_visible(max_visible.max(3));
        }
        let help = self.mode_help_text();
        self.status_line = if help.is_empty() {
            format!("resized to {}x{}", width, height)
        } else {
            format!("resized to {}x{} • {}", width, height, help)
        };
        if self.has_running_tool() {
            self.refresh_running_tool_visuals();
        }
        self.projection_dirty = true;
        self.refresh_projection(true);
        self.dirty = true;
    }

    pub fn on_key(&mut self, key: KeyEvent) {
        if matches!(key.kind, KeyEventKind::Release) {
            return;
        }

        match key.code {
            KeyCode::Char('c' | 'q') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.should_quit = true;
                self.dirty = true;
                return;
            }
            _ => {}
        }

        if (self.auth_dialog.is_some() || self.approval_dialog.is_some())
            && matches!(self.mode, TuiMode::Normal)
        {
            self.on_normal_key(key);
            return;
        }

        match key.code {
            KeyCode::Char('o' | 'O') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if matches!(self.mode, TuiMode::Normal) {
                    self.toggle_transcript_mode();
                } else {
                    self.set_tool_expand_status();
                    self.dirty = true;
                }
                return;
            }
            _ if is_clipboard_paste_shortcut(&key) => {
                if matches!(self.mode, TuiMode::Normal) {
                    if let Some((path, size)) = try_read_clipboard_image() {
                        self.on_image_attached(path, size);
                        self.suppress_next_paste_payload = true;
                    } else if let Some(text) = try_read_clipboard_text() {
                        self.on_paste(&text);
                    } else {
                        self.status_line = crate::ui_hints::CLIPBOARD_EMPTY_HINT.to_string();
                        self.dirty = true;
                    }
                }
                return;
            }
            KeyCode::Char('y') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.selection_mode = !self.selection_mode;
                if !self.selection_mode {
                    self.selection_anchor_row = None;
                    self.selection_anchor_col = None;
                    self.selection_focus_row = None;
                    self.selection_focus_col = None;
                }
                self.status_line = if self.selection_mode {
                    "selection mode enabled • drag in transcript to copy • Ctrl+Y exits selection"
                        .to_string()
                } else {
                    "selection mode off • Ctrl+Y enters drag-to-copy mode".to_string()
                };
                self.dirty = true;
                return;
            }
            _ => {}
        }

        match self.mode {
            TuiMode::Normal => self.on_normal_key(key),
            TuiMode::Transcript => self.on_transcript_key(key),
        }
    }
}
