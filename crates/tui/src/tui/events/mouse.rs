use super::*;

fn is_mouse_toggleable_block(block: &super::super::transcript::TranscriptBlock) -> bool {
    matches!(block.kind, BlockKind::ToolUse | BlockKind::ToolResult)
        || (block.kind == BlockKind::SystemNote
            && matches!(block.title.as_str(), "branch summary" | "compaction"))
}

impl TuiState {
    pub fn on_mouse(&mut self, event: MouseEvent) {
        let layout = self.current_layout();
        let in_transcript = event.row >= layout.transcript.y
            && event.row < layout.transcript.y.saturating_add(layout.transcript.height);

        if self.selection_mode {
            match event.kind {
                MouseEventKind::Down(MouseButton::Left) if in_transcript => {
                    if let Some(projected_row) = self.transcript_row_at_screen_row(event.row) {
                        let projected_col = self.transcript_col_at_screen_col(event.column);
                        self.selection_anchor_row = Some(projected_row);
                        self.selection_anchor_col = Some(projected_col);
                        self.selection_focus_row = Some(projected_row);
                        self.selection_focus_col = Some(projected_col);
                        self.dirty = true;
                    }
                    return;
                }
                MouseEventKind::Drag(MouseButton::Left) if in_transcript => {
                    if self.selection_anchor_row.is_some()
                        && let Some(projected_row) = self.transcript_row_at_screen_row(event.row)
                    {
                        let projected_col = self.transcript_col_at_screen_col(event.column);
                        self.selection_focus_row = Some(projected_row);
                        self.selection_focus_col = Some(projected_col);
                        self.dirty = true;
                    }
                    return;
                }
                MouseEventKind::Up(MouseButton::Left) => {
                    if self.selection_anchor_row.is_some() {
                        if in_transcript
                            && let Some(projected_row) =
                                self.transcript_row_at_screen_row(event.row)
                        {
                            let projected_col = self.transcript_col_at_screen_col(event.column);
                            self.selection_focus_row = Some(projected_row);
                            self.selection_focus_col = Some(projected_col);
                        }
                        self.copy_current_selection();
                        self.selection_anchor_row = None;
                        self.selection_anchor_col = None;
                        self.selection_focus_row = None;
                        self.selection_focus_col = None;
                        self.dirty = true;
                    }
                    return;
                }
                _ => {}
            }
        }

        match event.kind {
            MouseEventKind::ScrollUp if in_transcript => {
                self.viewport.scroll_up(3);
                if matches!(self.mode, TuiMode::Transcript) {
                    self.focus_first_visible_block();
                }
                self.status_line = self.transcript_scroll_status_line();
                self.dirty = true;
            }
            MouseEventKind::ScrollDown if in_transcript => {
                self.viewport.scroll_down(3);
                if matches!(self.mode, TuiMode::Transcript) {
                    if self.viewport.auto_follow {
                        self.mode = TuiMode::Normal;
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
                if let Some(block_id) = self.transcript_block_at_screen_row(event.row)
                    && self
                        .transcript
                        .block(block_id)
                        .is_some_and(is_mouse_toggleable_block)
                {
                    self.toggle_block(block_id);
                    return;
                }
                if let Some(block_id) = self.header_block_at_screen_row(event.row) {
                    self.toggle_block(block_id);
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => {}
            _ => {}
        }
    }

    fn transcript_row_at_screen_row(&self, row: u16) -> Option<usize> {
        let layout = self.current_layout();
        if row < layout.transcript.y || row >= layout.transcript.y + layout.transcript.height {
            return None;
        }

        let visible = self.viewport.visible_row_range();
        let local_row = row.saturating_sub(layout.transcript.y) as usize;
        let projected_row = visible.start + local_row;
        let row = self.projection.row(projected_row)?;
        if row.kind == ProjectedRowKind::Spacer {
            return None;
        }
        Some(projected_row)
    }

    fn transcript_block_at_screen_row(&self, row: u16) -> Option<BlockId> {
        let projected_row = self.transcript_row_at_screen_row(row)?;
        Some(self.projection.row(projected_row)?.block_id)
    }

    fn header_block_at_screen_row(&self, row: u16) -> Option<BlockId> {
        let block_id = self.transcript_block_at_screen_row(row)?;
        let projected_row = self.transcript_row_at_screen_row(row)?;
        let row = self.projection.row(projected_row)?;
        if row.kind != ProjectedRowKind::Header {
            return None;
        }
        Some(block_id)
    }

    fn transcript_col_at_screen_col(&self, column: u16) -> usize {
        let layout = self.current_layout();
        column.saturating_sub(layout.transcript.x) as usize
    }

    pub(crate) fn selection_span_for_row(&self, row_index: usize) -> Option<(usize, usize)> {
        let (Some(anchor_row), Some(anchor_col), Some(focus_row), Some(focus_col)) = (
            self.selection_anchor_row,
            self.selection_anchor_col,
            self.selection_focus_row,
            self.selection_focus_col,
        ) else {
            return None;
        };

        let ((start_row, start_col), (end_row, end_col)) =
            if (anchor_row, anchor_col) <= (focus_row, focus_col) {
                ((anchor_row, anchor_col), (focus_row, focus_col))
            } else {
                ((focus_row, focus_col), (anchor_row, anchor_col))
            };

        if row_index < start_row || row_index > end_row {
            return None;
        }

        let row = self.projection.row(row_index)?;
        if row.kind == ProjectedRowKind::Spacer {
            return None;
        }
        let plain = crate::utils::strip_ansi(&row.text).replace('\t', "   ");
        let row_width = crate::utils::visible_width(&plain);

        let (start, end_exclusive) = if start_row == end_row {
            (
                start_col.min(row_width),
                end_col.saturating_add(1).min(row_width),
            )
        } else if row_index == start_row {
            (start_col.min(row_width), row_width)
        } else if row_index == end_row {
            (0, end_col.saturating_add(1).min(row_width))
        } else {
            (0, row_width)
        };

        if start >= end_exclusive {
            None
        } else {
            Some((start, end_exclusive))
        }
    }

    fn copy_current_selection(&mut self) {
        let (Some(anchor_row), Some(focus_row)) =
            (self.selection_anchor_row, self.selection_focus_row)
        else {
            return;
        };
        let start = anchor_row.min(focus_row);
        let end = anchor_row.max(focus_row);
        let mut lines = Vec::new();
        for row_index in start..=end {
            let Some(row) = self.projection.row(row_index) else {
                continue;
            };
            if row.kind == ProjectedRowKind::Spacer {
                continue;
            }
            let Some((col_start, col_end)) = self.selection_span_for_row(row_index) else {
                continue;
            };

            let plain = crate::utils::strip_ansi(&row.text).replace('\t', "   ");
            lines.push(slice_by_visible_columns(&plain, col_start, col_end));
        }
        let text = lines.join("\n");
        if text.trim().is_empty() {
            self.status_line = "selection empty".to_string();
        } else {
            self.pending_clipboard_copy = Some(text);
            self.status_line = format!("Copied selection ({} line(s))", lines.len());
        }
    }
}

fn slice_by_visible_columns(text: &str, start: usize, end: usize) -> String {
    use crate::utils::char_width;

    if start >= end {
        return String::new();
    }

    let mut out = String::new();
    let mut col = 0usize;
    for ch in text.chars() {
        let cw = char_width(ch);
        let next = col + cw;
        if next <= start {
            col = next;
            continue;
        }
        if col >= end {
            break;
        }
        out.push(ch);
        col = next;
        if col >= end {
            break;
        }
    }
    out
}
