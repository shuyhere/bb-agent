//! Scrollback-based differential renderer.
//!
//! Matches pi-tui's rendering approach:
//! - Content is appended to the terminal scrollback buffer (not fullscreen)
//! - Only changed lines are redrawn
//! - All output wrapped in synchronized output (CSI ?2026h/l)
//! - Cursor positioned for IME via CURSOR_MARKER
//! - Hardware cursor can be shown/hidden

use crate::component::CURSOR_MARKER;
use crate::terminal::Terminal;
use crate::utils::visible_width;

/// Synchronized output escape sequences (prevents flicker).
const SYNC_BEGIN: &str = "\x1b[?2026h";
const SYNC_END: &str = "\x1b[?2026l";
/// Reset all styling + hyperlink.
const SEGMENT_RESET: &str = "\x1b[0m\x1b]8;;\x07";

pub struct Renderer {
    /// Lines previously rendered to the terminal.
    prev_lines: Vec<String>,
    /// Previous terminal width.
    prev_width: u16,
    /// Previous terminal height.
    prev_height: u16,
    /// Logical cursor row (end of rendered content).
    cursor_row: usize,
    /// Actual hardware cursor row (may differ for IME positioning).
    hw_cursor_row: usize,
    /// Whether we've done the first render.
    first_render: bool,
    /// Max lines ever rendered (tracks terminal working area).
    max_lines_rendered: usize,
    /// Show hardware cursor for IME.
    show_hw_cursor: bool,
}

impl Renderer {
    pub fn new() -> Self {
        Self {
            prev_lines: Vec::new(),
            prev_width: 0,
            prev_height: 0,
            cursor_row: 0,
            hw_cursor_row: 0,
            first_render: true,
            max_lines_rendered: 0,
            show_hw_cursor: true,
        }
    }

    pub fn set_show_hw_cursor(&mut self, show: bool) {
        self.show_hw_cursor = show;
    }

    /// Render new lines to terminal, updating only what changed.
    pub fn render(&mut self, new_lines: &[String], terminal: &mut dyn Terminal) {
        let width = terminal.columns();
        let height = terminal.rows();
        let width_changed = self.prev_width != 0 && self.prev_width != width;
        let height_changed = self.prev_height != 0 && self.prev_height != height;

        // Apply line resets
        let new_lines: Vec<String> = new_lines
            .iter()
            .map(|l| format!("{l}{SEGMENT_RESET}"))
            .collect();

        // Extract cursor position before rendering
        let cursor_pos = self.extract_cursor_position(&new_lines, height);

        // Strip CURSOR_MARKER from lines
        let new_lines: Vec<String> = new_lines
            .iter()
            .map(|l| l.replace(CURSOR_MARKER, ""))
            .collect();

        // First render — output everything without clearing
        if self.first_render && !width_changed && !height_changed {
            self.full_render(&new_lines, terminal, false);
            self.position_hw_cursor(cursor_pos, new_lines.len(), terminal);
            self.prev_lines = new_lines;
            self.prev_width = width;
            self.prev_height = height;
            return;
        }

        // Width or height changed — full clear + re-render
        if width_changed || height_changed {
            self.full_render(&new_lines, terminal, true);
            self.position_hw_cursor(cursor_pos, new_lines.len(), terminal);
            self.prev_lines = new_lines;
            self.prev_width = width;
            self.prev_height = height;
            return;
        }

        // Differential update
        self.diff_render(&new_lines, terminal);
        self.position_hw_cursor(cursor_pos, new_lines.len(), terminal);
        self.prev_lines = new_lines;
        self.prev_width = width;
        self.prev_height = height;
    }

    /// Force full re-render on next call.
    pub fn invalidate(&mut self) {
        self.prev_lines.clear();
        self.prev_width = 0;
        self.prev_height = 0;
        self.first_render = true;
        self.cursor_row = 0;
        self.hw_cursor_row = 0;
        self.max_lines_rendered = 0;
    }

    /// Full render: output all lines, optionally clearing first.
    fn full_render(&mut self, lines: &[String], terminal: &mut dyn Terminal, clear: bool) {
        let mut buf = String::new();
        buf.push_str(SYNC_BEGIN);
        if clear {
            buf.push_str("\x1b[2J\x1b[H\x1b[3J"); // clear screen + home + clear scrollback
        }
        for (i, line) in lines.iter().enumerate() {
            if i > 0 {
                buf.push_str("\r\n");
            }
            buf.push_str(line);
        }
        buf.push_str(SYNC_END);
        terminal.write(&buf);

        self.cursor_row = lines.len().saturating_sub(1);
        self.hw_cursor_row = self.cursor_row;
        self.first_render = false;
        if clear {
            self.max_lines_rendered = lines.len();
        } else {
            self.max_lines_rendered = self.max_lines_rendered.max(lines.len());
        }
    }

    /// Differential render: find first changed line, re-render from there.
    fn diff_render(&mut self, new_lines: &[String], terminal: &mut dyn Terminal) {
        let (first_changed, last_changed) = self.find_changed_range(new_lines);

        // No changes
        if first_changed.is_none() {
            return;
        }
        let first = first_changed.unwrap();
        let last = last_changed.unwrap();

        let mut buf = String::new();
        buf.push_str(SYNC_BEGIN);

        // Move cursor to first changed line
        let diff = self.hw_cursor_row as isize - first as isize;
        if diff > 0 {
            buf.push_str(&format!("\x1b[{}A", diff));
        } else if diff < 0 {
            buf.push_str(&format!("\x1b[{}B", -diff));
        }
        buf.push('\r');

        // Render changed lines
        let render_end = last.min(new_lines.len().saturating_sub(1));
        for i in first..=render_end {
            if i > first {
                buf.push_str("\r\n");
            }
            buf.push_str("\x1b[2K"); // clear line
            buf.push_str(&new_lines[i]);
        }

        let mut final_cursor_row = render_end;

        // Clear old lines that no longer exist
        if self.prev_lines.len() > new_lines.len() {
            // Move to end of new content if needed
            if render_end < new_lines.len().saturating_sub(1) {
                let move_down = new_lines.len() - 1 - render_end;
                buf.push_str(&format!("\x1b[{}B", move_down));
                final_cursor_row = new_lines.len() - 1;
            }
            let extra = self.prev_lines.len() - new_lines.len();
            for _ in 0..extra {
                buf.push_str("\r\n\x1b[2K");
            }
            // Move back up
            if extra > 0 {
                buf.push_str(&format!("\x1b[{}A", extra));
            }
        }

        buf.push_str(SYNC_END);
        terminal.write(&buf);

        self.cursor_row = new_lines.len().saturating_sub(1);
        self.hw_cursor_row = final_cursor_row;
        self.max_lines_rendered = self.max_lines_rendered.max(new_lines.len());
    }

    /// Find first and last changed line indices.
    fn find_changed_range(&self, new_lines: &[String]) -> (Option<usize>, Option<usize>) {
        let max = self.prev_lines.len().max(new_lines.len());
        let mut first = None;
        let mut last = None;

        for i in 0..max {
            let old = self.prev_lines.get(i).map(|s| s.as_str()).unwrap_or("");
            let new = new_lines.get(i).map(|s| s.as_str()).unwrap_or("");
            if old != new {
                if first.is_none() {
                    first = Some(i);
                }
                last = Some(i);
            }
        }

        // Include appended lines
        if new_lines.len() > self.prev_lines.len() {
            if first.is_none() {
                first = Some(self.prev_lines.len());
            }
            last = Some(new_lines.len() - 1);
        }

        (first, last)
    }

    /// Extract cursor position from rendered lines by searching for CURSOR_MARKER.
    fn extract_cursor_position(
        &self,
        lines: &[String],
        height: u16,
    ) -> Option<(usize, usize)> {
        let viewport_top = lines.len().saturating_sub(height as usize);
        for row in (viewport_top..lines.len()).rev() {
            if let Some(idx) = lines[row].find(CURSOR_MARKER) {
                let before = &lines[row][..idx];
                let col = visible_width(before);
                return Some((row, col));
            }
        }
        None
    }

    /// Position the hardware cursor for IME candidate window.
    fn position_hw_cursor(
        &mut self,
        cursor_pos: Option<(usize, usize)>,
        total_lines: usize,
        terminal: &mut dyn Terminal,
    ) {
        let Some((target_row, target_col)) = cursor_pos else {
            terminal.hide_cursor();
            return;
        };

        if total_lines == 0 {
            terminal.hide_cursor();
            return;
        }

        let target_row = target_row.min(total_lines - 1);
        let row_delta = target_row as isize - self.hw_cursor_row as isize;

        let mut buf = String::new();
        if row_delta > 0 {
            buf.push_str(&format!("\x1b[{}B", row_delta));
        } else if row_delta < 0 {
            buf.push_str(&format!("\x1b[{}A", -row_delta));
        }
        // Move to absolute column (1-indexed)
        buf.push_str(&format!("\x1b[{}G", target_col + 1));

        if !buf.is_empty() {
            terminal.write(&buf);
        }

        self.hw_cursor_row = target_row;

        if self.show_hw_cursor {
            terminal.show_cursor();
        } else {
            terminal.hide_cursor();
        }
    }
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::component::CURSOR_MARKER;

    #[test]
    fn test_extract_cursor_position() {
        let renderer = Renderer::new();
        let lines = vec![
            format!("line 0{SEGMENT_RESET}"),
            format!("hel{CURSOR_MARKER}lo{SEGMENT_RESET}"),
            format!("line 2{SEGMENT_RESET}"),
        ];
        let pos = renderer.extract_cursor_position(&lines, 24);
        assert_eq!(pos, Some((1, 3))); // row 1, col 3 ("hel" is 3 chars wide)
    }

    #[test]
    fn test_extract_cursor_no_marker() {
        let renderer = Renderer::new();
        let lines = vec![
            format!("line 0{SEGMENT_RESET}"),
            format!("line 1{SEGMENT_RESET}"),
        ];
        let pos = renderer.extract_cursor_position(&lines, 24);
        assert_eq!(pos, None);
    }

    #[test]
    fn test_find_changed_range_no_changes() {
        let mut renderer = Renderer::new();
        renderer.prev_lines = vec!["a".to_string(), "b".to_string()];
        let new = vec!["a".to_string(), "b".to_string()];
        let (first, last) = renderer.find_changed_range(&new);
        assert_eq!(first, None);
        assert_eq!(last, None);
    }

    #[test]
    fn test_find_changed_range_single_change() {
        let mut renderer = Renderer::new();
        renderer.prev_lines = vec!["a".to_string(), "b".to_string()];
        let new = vec!["a".to_string(), "c".to_string()];
        let (first, last) = renderer.find_changed_range(&new);
        assert_eq!(first, Some(1));
        assert_eq!(last, Some(1));
    }

    #[test]
    fn test_find_changed_range_append() {
        let mut renderer = Renderer::new();
        renderer.prev_lines = vec!["a".to_string()];
        let new = vec!["a".to_string(), "b".to_string()];
        let (first, last) = renderer.find_changed_range(&new);
        assert_eq!(first, Some(1));
        assert_eq!(last, Some(1));
    }
}
