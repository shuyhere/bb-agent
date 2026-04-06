use super::types::Editor;
use crate::component::{CURSOR_MARKER, Component};
use crate::utils::visible_width;
use crossterm::event::KeyEvent;

impl Editor {
    pub(super) fn render_line_with_selection_and_cursor(
        text: &str,
        cursor_pos: usize,
        hl_start: usize,
        hl_end: usize,
        marker: &str,
    ) -> String {
        // Split the text into regions: before-sel, sel, after-sel
        // The cursor is at cursor_pos bytes into text
        let result;

        // We have up to 5 segments depending on cursor position relative to selection
        let cp = cursor_pos.min(text.len());

        if cp < hl_start {
            // cursor before selection
            let before_cursor = &text[..cp];
            let cursor_to_hl = &text[cp..hl_start];
            let sel = &text[hl_start..hl_end];
            let after_hl = &text[hl_end..];
            let cursor_char: String = cursor_to_hl
                .chars()
                .next()
                .map(|c| c.to_string())
                .unwrap_or_default();
            let rest_before = &cursor_to_hl[cursor_char.len()..];
            result = format!(
                "{}{}\x1b[7m{}\x1b[0m{}\x1b[7m{}\x1b[0m{}",
                before_cursor, marker, cursor_char, rest_before, sel, after_hl
            );
        } else if cp >= hl_end {
            // cursor after selection
            let before_hl = &text[..hl_start];
            let sel = &text[hl_start..hl_end];
            let hl_to_cursor = &text[hl_end..cp];
            let after_cursor = &text[cp..];
            if !after_cursor.is_empty() {
                let cursor_char: String = after_cursor
                    .chars()
                    .next()
                    .map(|c| c.to_string())
                    .unwrap_or_default();
                let rest = &after_cursor[cursor_char.len()..];
                result = format!(
                    "{}\x1b[7m{}\x1b[0m{}{}\x1b[7m{}\x1b[0m{}",
                    before_hl, sel, hl_to_cursor, marker, cursor_char, rest
                );
            } else {
                result = format!(
                    "{}\x1b[7m{}\x1b[0m{}{}\x1b[7m \x1b[0m",
                    before_hl, sel, hl_to_cursor, marker
                );
            }
        } else {
            // cursor inside selection
            let before_hl = &text[..hl_start];
            let sel_before_cursor = &text[hl_start..cp];
            let sel_after_cursor = &text[cp..hl_end];
            let after_hl = &text[hl_end..];
            // Show entire selection highlighted, cursor marker inside
            result = format!(
                "{}\x1b[7m{}\x1b[0m{}\x1b[7m{}\x1b[0m{}",
                before_hl, sel_before_cursor, marker, sel_after_cursor, after_hl
            );
        }

        result
    }

    /// Word-wrap a line into chunks for display.
    pub(super) fn word_wrap_line(line: &str, max_width: usize) -> Vec<(String, usize, usize)> {
        if max_width == 0 {
            return vec![(String::new(), 0, 0)];
        }
        if line.is_empty() {
            return vec![(String::new(), 0, 0)];
        }

        let line_width = visible_width(line);
        if line_width <= max_width {
            return vec![(line.to_string(), 0, line.len())];
        }

        let mut chunks = Vec::new();
        let chars: Vec<char> = line.chars().collect();
        let mut pos = 0;

        while pos < chars.len() {
            let mut end = pos;
            let mut width = 0;
            let mut last_space = None;

            while end < chars.len() {
                let cw = unicode_width::UnicodeWidthChar::width(chars[end]).unwrap_or(0);
                if width + cw > max_width && end > pos {
                    break;
                }
                if chars[end] == ' ' {
                    last_space = Some(end);
                }
                width += cw;
                end += 1;
            }

            // Try to break at word boundary
            if let Some(last_space) = last_space
                && end < chars.len()
                && last_space > pos
            {
                end = last_space + 1;
            }

            let start_byte = chars[..pos].iter().collect::<String>().len();
            let end_byte = chars[..end].iter().collect::<String>().len();
            let text: String = chars[pos..end].iter().collect();
            chunks.push((text, start_byte, end_byte));
            pos = end;
        }

        if chunks.is_empty() {
            chunks.push((String::new(), 0, 0));
        }

        chunks
    }

    /// Build layout lines for rendering.
    fn layout_text(&self, content_width: usize) -> Vec<LayoutLine> {
        let mut layout = Vec::new();

        if self.state.lines.len() == 1 && self.state.lines[0].is_empty() {
            layout.push(LayoutLine {
                text: String::new(),
                has_cursor: true,
                cursor_pos: Some(0),
                line_index: 0,
                byte_start: 0,
            });
            return layout;
        }

        for (i, line) in self.state.lines.iter().enumerate() {
            let is_current = i == self.state.cursor_line;
            let chunks = Self::word_wrap_line(line, content_width);

            for (ci, (text, start_byte, end_byte)) in chunks.iter().enumerate() {
                let is_last_chunk = ci == chunks.len() - 1;

                if is_current {
                    let col = self.state.cursor_col;
                    let in_chunk = if is_last_chunk {
                        col >= *start_byte
                    } else {
                        col >= *start_byte && col < *end_byte
                    };

                    if in_chunk {
                        let adjusted = col - start_byte;
                        layout.push(LayoutLine {
                            text: text.clone(),
                            has_cursor: true,
                            cursor_pos: Some(adjusted.min(text.len())),
                            line_index: i,
                            byte_start: *start_byte,
                        });
                    } else {
                        layout.push(LayoutLine {
                            text: text.clone(),
                            has_cursor: false,
                            cursor_pos: None,
                            line_index: i,
                            byte_start: *start_byte,
                        });
                    }
                } else {
                    layout.push(LayoutLine {
                        text: text.clone(),
                        has_cursor: false,
                        cursor_pos: None,
                        line_index: i,
                        byte_start: *start_byte,
                    });
                }
            }
        }

        layout
    }
}

struct LayoutLine {
    text: String,
    has_cursor: bool,
    cursor_pos: Option<usize>,
    /// Which logical line index this layout line comes from.
    line_index: usize,
    /// Byte offset in the original line where this chunk starts.
    byte_start: usize,
}

impl Component for Editor {
    crate::impl_as_any!();

    fn render(&self, width: u16) -> Vec<String> {
        let w = width as usize;
        if w < 3 {
            return vec!["─".repeat(w)];
        }

        let content_width = w.saturating_sub(1); // reserve 1 for cursor at end
        let layout_lines = self.layout_text(content_width);

        // Calculate max visible lines: 30% of terminal height, minimum 5
        let max_visible = (self.terminal_rows as usize * 30 / 100).max(5);

        // Find cursor line in layout
        let cursor_line_idx = layout_lines.iter().position(|l| l.has_cursor).unwrap_or(0);

        // Adjust scroll
        let mut scroll = self.scroll_offset;
        if cursor_line_idx < scroll {
            scroll = cursor_line_idx;
        } else if cursor_line_idx >= scroll + max_visible {
            scroll = cursor_line_idx - max_visible + 1;
        }
        let max_scroll = layout_lines.len().saturating_sub(max_visible);
        scroll = scroll.min(max_scroll);

        let visible = &layout_lines[scroll..layout_lines.len().min(scroll + max_visible)];

        let border = format!("{}{}{}", self.border_color, "─".repeat(w), "\x1b[0m");
        let mut result = Vec::new();

        // Top border (with scroll indicator if scrolled)
        if scroll > 0 {
            let indicator = format!("─── ↑ {} more ", scroll);
            let remaining = w.saturating_sub(visible_width(&indicator));
            result.push(format!(
                "{}{}{}{}",
                self.border_color,
                indicator,
                "─".repeat(remaining),
                "\x1b[0m"
            ));
        } else {
            result.push(border.clone());
        }

        // Compute selection range for highlighting
        let sel_range = self.selection_range();

        // Content lines
        let emit_cursor = self.focused;
        for ll in visible {
            let mut display = ll.text.clone();

            // Apply selection highlighting
            if let Some(((sl, sc), (el, ec))) = sel_range {
                let li = ll.line_index;
                // Check if this layout line overlaps the selection
                if li >= sl && li <= el {
                    // Compute selection byte range within this chunk
                    let chunk_start = ll.byte_start;
                    let sel_start_in_line = if li == sl { sc } else { 0 };
                    let sel_end_in_line = if li == el {
                        ec
                    } else {
                        self.state.lines[li].len()
                    };

                    // Clamp to this chunk
                    let hl_start = sel_start_in_line
                        .max(chunk_start)
                        .saturating_sub(chunk_start);
                    let hl_end = sel_end_in_line
                        .min(chunk_start + ll.text.len())
                        .saturating_sub(chunk_start);

                    if hl_start < hl_end && hl_end <= ll.text.len() {
                        let before_sel = &ll.text[..hl_start];
                        let sel_part = &ll.text[hl_start..hl_end];
                        let after_sel = &ll.text[hl_end..];
                        display = format!("{}\x1b[7m{}\x1b[0m{}", before_sel, sel_part, after_sel);
                    }
                }
            }

            if ll.has_cursor
                && emit_cursor
                && let Some(pos) = ll.cursor_pos
            {
                // Re-render with cursor marker (on top of any selection)
                // We need to work from the raw text for cursor positioning
                let raw_before = &ll.text[..pos.min(ll.text.len())];
                let raw_after = &ll.text[pos.min(ll.text.len())..];

                let marker = CURSOR_MARKER;

                // Build display with both selection highlight and cursor
                if let Some(((sl, sc), (el, ec))) = sel_range {
                    let li = ll.line_index;
                    if li >= sl && li <= el {
                        let chunk_start = ll.byte_start;
                        let sel_start_in_line = if li == sl { sc } else { 0 };
                        let sel_end_in_line = if li == el {
                            ec
                        } else {
                            self.state.lines[li].len()
                        };
                        let hl_start = sel_start_in_line
                            .max(chunk_start)
                            .saturating_sub(chunk_start);
                        let hl_end = sel_end_in_line
                            .min(chunk_start + ll.text.len())
                            .saturating_sub(chunk_start);

                        if hl_start < hl_end && hl_end <= ll.text.len() {
                            // Build the line char by char with selection and cursor
                            display = Self::render_line_with_selection_and_cursor(
                                &ll.text, pos, hl_start, hl_end, marker,
                            );
                        } else if !raw_after.is_empty() {
                            let first_char: String = raw_after
                                .chars()
                                .next()
                                .map(|c| c.to_string())
                                .unwrap_or_default();
                            let rest = &raw_after[first_char.len()..];
                            display = format!(
                                "{}{}\x1b[7m{}\x1b[0m{}",
                                raw_before, marker, first_char, rest
                            );
                        } else {
                            display = format!("{}{}\x1b[7m \x1b[0m", raw_before, marker);
                        }
                    } else if !raw_after.is_empty() {
                        let first_char: String = raw_after
                            .chars()
                            .next()
                            .map(|c| c.to_string())
                            .unwrap_or_default();
                        let rest = &raw_after[first_char.len()..];
                        display = format!(
                            "{}{}\x1b[7m{}\x1b[0m{}",
                            raw_before, marker, first_char, rest
                        );
                    } else {
                        display = format!("{}{}\x1b[7m \x1b[0m", raw_before, marker);
                    }
                } else if !raw_after.is_empty() {
                    let first_char: String = raw_after
                        .chars()
                        .next()
                        .map(|c| c.to_string())
                        .unwrap_or_default();
                    let rest = &raw_after[first_char.len()..];
                    display = format!(
                        "{}{}\x1b[7m{}\x1b[0m{}",
                        raw_before, marker, first_char, rest
                    );
                } else {
                    display = format!("{}{}\x1b[7m \x1b[0m", raw_before, marker);
                }
            }

            // Pad to full width
            let vw = visible_width(&display);
            let padding = if w > vw {
                " ".repeat(w - vw)
            } else {
                String::new()
            };
            result.push(format!("{}{}", display, padding));
        }

        // Bottom border (with scroll indicator if more below)
        let lines_below = layout_lines.len().saturating_sub(scroll + visible.len());
        if lines_below > 0 {
            let indicator = format!("─── ↓ {} more ", lines_below);
            let remaining = w.saturating_sub(visible_width(&indicator));
            result.push(format!(
                "{}{}{}{}",
                self.border_color,
                indicator,
                "─".repeat(remaining),
                "\x1b[0m"
            ));
        } else {
            result.push(border.clone());
        }

        if let Some(menu) = &self.slash_menu {
            let menu_lines = menu.render(width);
            result.extend(menu_lines);
        }

        if let Some(menu) = &self.file_menu {
            let menu_lines = menu.render(width);
            result.extend(menu_lines);
        }

        result
    }

    fn handle_input(&mut self, key: &KeyEvent) {
        self.handle_key_event(key);
    }

    fn handle_raw_input(&mut self, data: &str) {
        self.handle_paste_input(data);
    }

    fn invalidate(&mut self) {}
}
