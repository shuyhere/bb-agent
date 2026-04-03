use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};

use crate::select_list::SelectAction;

use super::{
    layout::Size,
    projection::ProjectedRowKind,
    runtime::FullscreenState,
    transcript::{BlockId, BlockKind},
    types::{FullscreenMode, FullscreenSubmission},
};

impl FullscreenState {
    pub fn on_tick(&mut self) {
        self.tick_count = self.tick_count.wrapping_add(1);
        if self.should_animate_status() {
            self.dirty = true;
        }
    }

    pub fn on_resize(&mut self, width: u16, height: u16) {
        self.size = Size { width, height };
        let help = self.mode_help_text();
        self.status_line = if help.is_empty() {
            format!("resized to {}x{}", width, height)
        } else {
            format!("resized to {}x{} • {}", width, height, help)
        };
        self.projection_dirty = true;
        self.refresh_projection(true);
        self.dirty = true;
    }

    pub fn on_paste(&mut self, text: &str) {
        match self.mode {
            FullscreenMode::Normal => {
                self.insert_str(text);
            }
            FullscreenMode::Search => {
                // Search mode is no longer entered from transcript.
                // If somehow active, treat paste as returning to Normal.
                self.mode = FullscreenMode::Normal;
                self.viewport.auto_follow = true;
                self.status_line = self.mode_help_text();
                self.insert_str(text);
            }
            FullscreenMode::Transcript => {
                self.status_line =
                    "paste is ignored while transcript navigation is active".to_string();
                self.dirty = true;
            }
        }
    }

    pub fn on_key(&mut self, key: KeyEvent) {
        if matches!(key.kind, KeyEventKind::Release) {
            return;
        }

        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.should_quit = true;
                self.dirty = true;
                return;
            }
            KeyCode::Char('q') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.should_quit = true;
                self.dirty = true;
                return;
            }
            KeyCode::Char('o') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if matches!(self.mode, FullscreenMode::Transcript)
                    && self
                        .focused_block
                        .and_then(|block_id| self.transcript.block(block_id))
                        .is_some_and(|block| block.kind == BlockKind::ToolUse)
                {
                    self.toggle_tool_output_expansion();
                } else {
                    self.toggle_transcript_mode();
                }
                return;
            }
            _ => {}
        }

        match self.mode {
            FullscreenMode::Normal => self.on_normal_key(key),
            FullscreenMode::Transcript => self.on_transcript_key(key),
            // Search mode is no longer reachable from transcript.
            // If somehow active, Esc returns to Normal.
            FullscreenMode::Search => {
                self.mode = FullscreenMode::Normal;
                self.viewport.auto_follow = true;
                self.status_line = self.mode_help_text();
                self.dirty = true;
            }
        }
    }

    pub fn on_mouse(&mut self, event: MouseEvent) {
        let layout = self.current_layout();
        let in_transcript = event.row >= layout.transcript.y
            && event.row < layout.transcript.y.saturating_add(layout.transcript.height);

        match event.kind {
            MouseEventKind::ScrollUp if in_transcript => {
                self.viewport.scroll_up(3);
                // Stay in current mode — do NOT force Transcript mode.
                // User can keep typing while scrolling. Ctrl+O enters
                // Transcript mode deliberately for navigation.
                if matches!(self.mode, FullscreenMode::Transcript) {
                    self.focus_first_visible_block();
                }
                self.status_line = self.transcript_scroll_status_line();
                self.dirty = true;
            }
            MouseEventKind::ScrollDown if in_transcript => {
                self.viewport.scroll_down(3);
                if matches!(self.mode, FullscreenMode::Transcript) {
                    if self.viewport.auto_follow {
                        // Scrolled back to bottom — exit transcript mode
                        self.mode = FullscreenMode::Normal;
                        self.status_line = self.mode_help_text();
                    } else {
                        self.focus_last_visible_block();
                        self.status_line = self.transcript_scroll_status_line();
                    }
                } else {
                    self.status_line = self.transcript_scroll_status_line();
                }
                self.dirty = true;
            }
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(block_id) = self.header_block_at_screen_row(event.row) {
                    // Toggle the block but do NOT switch to Transcript mode.
                    // The user clicked to expand/collapse, not to navigate.
                    self.toggle_block(block_id);
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => {}
            _ => {}
        }
    }

    fn on_normal_key(&mut self, key: KeyEvent) {
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
                    self.pending_submissions.push_back(FullscreenSubmission::MenuSelection {
                        menu_id,
                        value,
                    });
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

        match key.code {
            KeyCode::Esc => {
                self.should_quit = true;
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

    fn on_transcript_key(&mut self, key: KeyEvent) {
        match (key.code, key.modifiers) {
            (KeyCode::Esc, KeyModifiers::NONE) => {
                self.mode = FullscreenMode::Normal;
                self.viewport.auto_follow = true;
                self.status_line = self.mode_help_text();
                self.dirty = true;
            }
            (KeyCode::Down, KeyModifiers::NONE) | (KeyCode::Char('j'), KeyModifiers::NONE) => {
                self.move_focus(1);
            }
            (KeyCode::Up, KeyModifiers::NONE) | (KeyCode::Char('k'), KeyModifiers::NONE) => {
                self.move_focus(-1);
            }
            (KeyCode::PageDown, KeyModifiers::NONE) => {
                self.page_move(1);
            }
            (KeyCode::PageUp, KeyModifiers::NONE) => {
                self.page_move(-1);
            }
            (KeyCode::Home, KeyModifiers::NONE) | (KeyCode::Char('g'), KeyModifiers::NONE) => {
                self.focus_first();
            }
            (KeyCode::End, KeyModifiers::NONE) | (KeyCode::Char('G'), KeyModifiers::SHIFT) => {
                self.focus_last();
            }
            (KeyCode::Enter, KeyModifiers::NONE) | (KeyCode::Char(' '), KeyModifiers::NONE) => {
                self.toggle_focused_block();
            }
            (KeyCode::Char('o'), KeyModifiers::NONE) => {
                self.expand_focused_block();
            }
            (KeyCode::Char('c'), KeyModifiers::NONE) => {
                self.collapse_focused_block();
            }
            // '/' returns to Normal mode and inserts into the input
            // (which opens the slash command menu). Transcript search is
            // not available here — use /tree for session navigation.
            (KeyCode::Char('/'), KeyModifiers::NONE) => {
                self.mode = FullscreenMode::Normal;
                self.viewport.auto_follow = true;
                self.status_line = self.mode_help_text();
                self.insert_char('/');
            }
            _ => {}
        }
    }

    fn toggle_transcript_mode(&mut self) {
        self.mode = match self.mode {
            FullscreenMode::Normal => FullscreenMode::Transcript,
            FullscreenMode::Transcript | FullscreenMode::Search => FullscreenMode::Normal,
        };

        if matches!(self.mode, FullscreenMode::Transcript) && self.focused_block.is_none()
        {
            self.focused_block = if self.viewport.auto_follow {
                self.last_focusable_block().or_else(|| self.default_focus_block())
            } else {
                self.default_focus_block()
            };
            self.sync_focus_tracking();
            if !self.viewport.auto_follow {
                self.ensure_focus_visible();
            }
        }

        self.status_line = self.mode_help_text();
        self.dirty = true;
    }

    fn should_animate_status(&self) -> bool {
        matches!(self.mode, FullscreenMode::Normal)
            && (self.has_active_turn() || !self.pending_submissions.is_empty())
    }

    fn transcript_scroll_status_line(&self) -> String {
        let follow = if self.viewport.auto_follow {
            "follow on"
        } else {
            "follow off"
        };
        format!(
            "transcript row {} • {follow} • j/k navigate • Enter toggles",
            self.viewport.viewport_top
        )
    }

    fn toggle_focused_block(&mut self) {
        let Some(block_id) = self.focused_block else {
            return;
        };
        self.toggle_block(block_id);
    }

    fn expand_focused_block(&mut self) {
        let Some(block_id) = self.focused_block else {
            return;
        };
        self.set_block_collapsed(block_id, false, "expanded");
    }

    fn collapse_focused_block(&mut self) {
        let Some(block_id) = self.focused_block else {
            return;
        };
        self.set_block_collapsed(block_id, true, "collapsed");
    }

    fn toggle_block(&mut self, block_id: BlockId) {
        let Some(block) = self.transcript.block(block_id).cloned() else {
            return;
        };
        if !(block.expandable || !block.children.is_empty()) {
            self.status_line = format!(
                "focused {} block is not expandable",
                self.block_label(block_id)
            );
            self.dirty = true;
            return;
        }

        let next_collapsed = !block.collapsed;
        let action = if next_collapsed {
            "collapsed"
        } else {
            "expanded"
        };
        self.set_block_collapsed(block_id, next_collapsed, action);
    }

    fn set_block_collapsed(&mut self, block_id: BlockId, collapsed: bool, action: &str) {
        let Some(block) = self.transcript.block(block_id).cloned() else {
            return;
        };
        if !(block.expandable || !block.children.is_empty()) {
            self.status_line = format!(
                "focused {} block is not expandable",
                self.block_label(block_id)
            );
            self.dirty = true;
            return;
        }
        if block.collapsed == collapsed {
            self.status_line = format!("{} already {}", self.block_label(block_id), action);
            self.dirty = true;
            return;
        }

        if self.transcript.set_collapsed(block_id, collapsed).is_err() {
            return;
        }

        self.projection_dirty = true;
        self.refresh_projection(true);
        self.focus_block(block_id);
        self.status_line = format!("{} {}", action, self.block_label(block_id));
    }

    fn block_label(&self, block_id: BlockId) -> String {
        self.transcript
            .block(block_id)
            .map(|block| {
                if block.title.trim().is_empty() {
                    format!("block {}", block_id.get())
                } else {
                    format!("“{}”", block.title)
                }
            })
            .unwrap_or_else(|| format!("block {}", block_id.get()))
    }

    fn header_block_at_screen_row(&self, row: u16) -> Option<BlockId> {
        let layout = self.current_layout();
        if row < layout.transcript.y || row >= layout.transcript.y + layout.transcript.height {
            return None;
        }

        let visible = self.viewport.visible_row_range();
        let local_row = row.saturating_sub(layout.transcript.y) as usize;
        let projected_row = visible.start + local_row;
        let row = self.projection.row(projected_row)?;
        if row.kind != ProjectedRowKind::Header {
            return None;
        }
        Some(row.block_id)
    }
}
