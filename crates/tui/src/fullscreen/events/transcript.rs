use super::*;

impl FullscreenState {
    pub(super) fn on_transcript_key(&mut self, key: KeyEvent) {
        match (key.code, key.modifiers) {
            (KeyCode::Esc, KeyModifiers::NONE) => {
                self.mode = FullscreenMode::Normal;
                self.viewport.auto_follow = true;
                self.status_line = self.mode_help_text();
                self.dirty = true;
            }
            (KeyCode::Down, KeyModifiers::NONE) | (KeyCode::Char('j'), KeyModifiers::NONE) => {
                self.move_focus(1);
                self.set_tool_expand_status();
            }
            (KeyCode::Up, KeyModifiers::NONE) | (KeyCode::Char('k'), KeyModifiers::NONE) => {
                self.move_focus(-1);
                self.set_tool_expand_status();
            }
            (KeyCode::PageDown, KeyModifiers::NONE) => {
                self.page_move(1);
                self.set_tool_expand_status();
            }
            (KeyCode::PageUp, KeyModifiers::NONE) => {
                self.page_move(-1);
                self.set_tool_expand_status();
            }
            (KeyCode::Home, KeyModifiers::NONE) | (KeyCode::Char('g'), KeyModifiers::NONE) => {
                self.focus_first();
                self.set_tool_expand_status();
            }
            (KeyCode::End, KeyModifiers::NONE) | (KeyCode::Char('G'), KeyModifiers::SHIFT) => {
                self.focus_last();
                self.set_tool_expand_status();
            }
            (KeyCode::Enter, KeyModifiers::NONE)
            | (KeyCode::Char('m'), KeyModifiers::CONTROL)
            | (KeyCode::Char('j'), KeyModifiers::CONTROL) => {
                self.toggle_tool_output_expansion();
                self.set_tool_expand_status();
            }
            _ => {}
        }
    }

    pub(super) fn toggle_transcript_mode(&mut self) {
        self.mode = match self.mode {
            FullscreenMode::Normal => FullscreenMode::Transcript,
            FullscreenMode::Transcript => FullscreenMode::Normal,
        };

        if matches!(self.mode, FullscreenMode::Transcript) {
            self.viewport.auto_follow = false;
            self.focused_block = self
                .visible_tool_use_blocks()
                .last()
                .copied()
                .or_else(|| self.last_focusable_block())
                .or_else(|| self.first_focusable_block());
            self.sync_focus_tracking();
            self.ensure_focus_visible();
        }

        self.set_tool_expand_status();
        self.dirty = true;
    }

    pub(super) fn set_tool_expand_status(&mut self) {
        if !matches!(self.mode, FullscreenMode::Transcript) {
            self.status_line = self.mode_help_text();
            return;
        }

        let selected = self
            .focused_block
            .and_then(|block_id| self.transcript.block(block_id))
            .map(|block| {
                block
                    .title
                    .split(" • ")
                    .next()
                    .unwrap_or(block.title.as_str())
                    .trim()
                    .to_string()
            })
            .filter(|title| !title.is_empty())
            .unwrap_or_else(|| "(no tool selected)".to_string());

        self.status_line = format!(
            "tool expand • selected: {selected} • j/k or ↑/↓ move • Enter expand/collapse • Esc returns"
        );
    }

    pub(super) fn should_animate_status(&self) -> bool {
        matches!(self.mode, FullscreenMode::Normal)
            && (self.has_active_turn() || !self.pending_submissions.is_empty())
    }

    pub(super) fn transcript_scroll_status_line(&self) -> String {
        let follow = if self.viewport.auto_follow {
            "follow on"
        } else {
            "follow off • Esc to jump to latest"
        };
        format!("transcript row {} • {follow}", self.viewport.viewport_top)
    }

    pub(super) fn toggle_block(&mut self, block_id: BlockId) {
        let Some(block) = self.transcript.block(block_id).cloned() else {
            return;
        };

        if block.kind == super::BlockKind::ToolUse || block.kind == super::BlockKind::ToolResult {
            self.focused_block = Some(block_id);
            self.toggle_tool_output_expansion();
            return;
        }

        if !block.expandable && block.children.is_empty() {
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
        if !block.expandable && block.children.is_empty() {
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
}
