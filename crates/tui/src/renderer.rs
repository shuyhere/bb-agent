//! Scrollback-based differential renderer — ported from pi's tui.ts doRender.
//!
//! Key concepts from pi:
//! - `hardwareCursorRow`: absolute row in scrollback buffer where cursor is
//! - `viewportTop`: which scrollback row is at the top of the visible terminal
//! - `computeLineDiff`: converts absolute row to relative cursor movement
//! - Only changed lines (firstChanged..lastChanged) are redrawn
//! - Synchronized output (CSI ?2026h/l) prevents flicker

use crate::component::CURSOR_MARKER;
use crate::terminal::Terminal;
use crate::utils::visible_width;

const SYNC_BEGIN: &str = "\x1b[?2026h";
const SYNC_END: &str = "\x1b[?2026l";
const SEGMENT_RESET: &str = "\x1b[0m\x1b]8;;\x07";
/// OSC 133;A — shell prompt marker re-used as a line-boundary reset.
#[allow(dead_code)]
const LINE_RESET_MARKER: &str = "\x1b]133;A\x07";

pub struct Renderer {
    prev_lines: Vec<String>,
    prev_width: u16,
    prev_height: u16,
    /// Absolute row in scrollback where cursor currently is.
    hw_cursor_row: usize,
    /// Which scrollback row is at the top of the visible terminal.
    prev_viewport_top: usize,
    max_lines_rendered: usize,
    /// When true, do a full clear+render when content shrinks (fewer lines
    /// than the previous high-water mark).
    pub clear_on_shrink: bool,
    /// When true, show the terminal cursor and position it at CURSOR_MARKER.
    pub show_hardware_cursor: bool,
}

impl Renderer {
    pub fn new() -> Self {
        Self {
            prev_lines: Vec::new(),
            prev_width: 0,
            prev_height: 0,
            hw_cursor_row: 0,
            prev_viewport_top: 0,
            max_lines_rendered: 0,
            clear_on_shrink: false,
            show_hardware_cursor: false,
        }
    }

    /// Enable/disable clearing leftover rows when content shrinks.
    pub fn set_clear_on_shrink(&mut self, enabled: bool) {
        self.clear_on_shrink = enabled;
    }

    /// Enable/disable hardware cursor positioning at CURSOR_MARKER.
    pub fn set_show_hardware_cursor(&mut self, enabled: bool) {
        self.show_hardware_cursor = enabled;
    }

    pub fn invalidate(&mut self) {
        self.prev_lines.clear();
        self.prev_width = 0; // triggers width_changed -> full render
    }

    pub fn render(&mut self, new_lines: &[String], terminal: &mut dyn Terminal) {
        let width = terminal.columns();
        let height = terminal.rows();
        let height_usize = height as usize;
        let width_changed = self.prev_width != 0 && self.prev_width != width;
        let height_changed = self.prev_height != 0 && self.prev_height != height;

        // Apply line resets (SEGMENT_RESET prevents colour bleed across lines)
        let new_lines: Vec<String> = Self::apply_line_resets(new_lines);

        // Width overflow protection — truncate lines exceeding terminal width
        let width_usize = width as usize;
        let new_lines: Vec<String> = new_lines
            .into_iter()
            .map(|l| {
                if visible_width(&l) > width_usize {
                    #[cfg(debug_assertions)]
                    eprintln!(
                        "[renderer] line exceeds terminal width ({} > {}), truncating",
                        visible_width(&l),
                        width_usize
                    );
                    crate::utils::truncate_to_width(&l, width_usize)
                } else {
                    l
                }
            })
            .collect();

        // Extract cursor position
        let cursor_pos = self.find_cursor(&new_lines);

        // Strip CURSOR_MARKER
        let new_lines: Vec<String> = new_lines
            .iter()
            .map(|l| l.replace(CURSOR_MARKER, ""))
            .collect();

        // --- Full render cases ---

        // clear_on_shrink: content shrunk below the high-water mark
        let needs_shrink_clear = self.clear_on_shrink
            && new_lines.len() < self.max_lines_rendered
            && !self.prev_lines.is_empty();

        // First render
        if self.prev_lines.is_empty() && !width_changed && !height_changed {
            self.full_render(&new_lines, terminal, false);
            self.position_hardware_cursor(cursor_pos, terminal);
            self.save_state(&new_lines, width, height);
            return;
        }

        // Width changed
        if width_changed {
            self.full_render(&new_lines, terminal, true);
            self.position_hardware_cursor(cursor_pos, terminal);
            self.save_state(&new_lines, width, height);
            return;
        }

        // Height changed
        if height_changed {
            // Recalculate viewport top after height change
            let prev_buffer_len = self.prev_viewport_top + self.prev_height as usize;
            self.prev_viewport_top = prev_buffer_len.saturating_sub(height_usize);
            self.full_render(&new_lines, terminal, true);
            self.position_hardware_cursor(cursor_pos, terminal);
            self.save_state(&new_lines, width, height);
            return;
        }

        // Content shrunk and clear_on_shrink is enabled
        if needs_shrink_clear {
            self.full_render(&new_lines, terminal, true);
            self.position_hardware_cursor(cursor_pos, terminal);
            self.save_state(&new_lines, width, height);
            return;
        }

        // --- Differential render ---
        self.diff_render(&new_lines, height_usize, terminal);
        self.position_hardware_cursor(cursor_pos, terminal);
        self.save_state(&new_lines, width, height);
    }

    /// Append SEGMENT_RESET (ANSI reset + hyperlink close) to each line to
    /// prevent colour/style bleed across line boundaries.
    fn apply_line_resets(lines: &[String]) -> Vec<String> {
        lines
            .iter()
            .map(|l| format!("{l}{SEGMENT_RESET}"))
            .collect()
    }

    fn full_render(&mut self, lines: &[String], terminal: &mut dyn Terminal, clear: bool) {
        let height = terminal.rows() as usize;
        let mut buf = String::new();
        buf.push_str(SYNC_BEGIN);
        if clear {
            buf.push_str("\x1b[2J\x1b[H"); // Clear screen + home (no \x1b[3J = preserve scrollback)
        }
        for (i, line) in lines.iter().enumerate() {
            if i > 0 {
                buf.push_str("\r\n");
            }
            buf.push_str(line);
        }
        buf.push_str(SYNC_END);
        terminal.write(&buf);

        self.hw_cursor_row = lines.len().saturating_sub(1);
        if clear {
            self.max_lines_rendered = lines.len();
        } else {
            self.max_lines_rendered = self.max_lines_rendered.max(lines.len());
        }
        let buffer_len = height.max(lines.len());
        self.prev_viewport_top = buffer_len.saturating_sub(height);
    }

    fn diff_render(&mut self, new_lines: &[String], height: usize, terminal: &mut dyn Terminal) {
        let prev_count = self.prev_lines.len();
        let new_count = new_lines.len();
        let max_lines = prev_count.max(new_count);

        // Find first and last changed lines
        let mut first_changed: Option<usize> = None;
        let mut last_changed: Option<usize> = None;
        for i in 0..max_lines {
            let old = self.prev_lines.get(i).map(|s| s.as_str()).unwrap_or("");
            let new = new_lines.get(i).map(|s| s.as_str()).unwrap_or("");
            if old != new {
                if first_changed.is_none() {
                    first_changed = Some(i);
                }
                last_changed = Some(i);
            }
        }

        // Handle appended lines
        if new_count > prev_count {
            if first_changed.is_none() {
                first_changed = Some(prev_count);
            }
            last_changed = Some(new_count - 1);
        }

        // No changes
        if first_changed.is_none() {
            return;
        }

        let first = first_changed.unwrap();
        let last = last_changed.unwrap();
        let append_start = new_count > prev_count && first == prev_count && first > 0;

        let mut buf = String::new();
        buf.push_str(SYNC_BEGIN);

        let mut hw_row = self.hw_cursor_row;
        let mut viewport_top = self.prev_viewport_top;

        // Pi's computeLineDiff: convert absolute row to relative cursor movement
        let compute_line_diff = |target: usize, cur_hw: usize, prev_vt: usize, cur_vt: usize| -> isize {
            let current_screen = cur_hw as isize - prev_vt as isize;
            let target_screen = target as isize - cur_vt as isize;
            target_screen - current_screen
        };

        let move_target = if append_start { first - 1 } else { first };

        // If target is below visible viewport, scroll down
        let prev_viewport_bottom = self.prev_viewport_top + height.saturating_sub(1);
        if move_target > prev_viewport_bottom {
            let current_screen_row = hw_row.saturating_sub(self.prev_viewport_top).min(height - 1);
            let move_to_bottom = (height - 1).saturating_sub(current_screen_row);
            if move_to_bottom > 0 {
                buf.push_str(&format!("\x1b[{}B", move_to_bottom));
            }
            let scroll = move_target - prev_viewport_bottom;
            for _ in 0..scroll {
                buf.push_str("\r\n");
            }
            viewport_top += scroll;
            hw_row = move_target;
        }

        // Move cursor to target line
        let line_diff = compute_line_diff(move_target, hw_row, self.prev_viewport_top, viewport_top);
        if line_diff > 0 {
            buf.push_str(&format!("\x1b[{}B", line_diff));
        } else if line_diff < 0 {
            buf.push_str(&format!("\x1b[{}A", -line_diff));
        }

        buf.push_str(if append_start { "\r\n" } else { "\r" });

        // Render only changed lines (first..=last)
        let render_end = last.min(new_count.saturating_sub(1));
        for i in first..=render_end {
            if i > first {
                buf.push_str("\r\n");
            }
            buf.push_str("\x1b[2K");
            buf.push_str(&new_lines[i]);
        }

        let mut final_cursor_row = render_end;

        // Clear extra old lines if content shrunk
        if prev_count > new_count {
            if render_end < new_count.saturating_sub(1) {
                let move_down = new_count - 1 - render_end;
                buf.push_str(&format!("\x1b[{}B", move_down));
                final_cursor_row = new_count - 1;
            }
            let extra = prev_count - new_count;
            for _ in 0..extra {
                buf.push_str("\r\n\x1b[2K");
            }
            if extra > 0 {
                buf.push_str(&format!("\x1b[{}A", extra));
            }
        }

        buf.push_str(SYNC_END);
        terminal.write(&buf);

        self.hw_cursor_row = final_cursor_row;
        self.max_lines_rendered = self.max_lines_rendered.max(new_count);
        self.prev_viewport_top = viewport_top.max(
            final_cursor_row.saturating_sub(height.saturating_sub(1))
        );
    }

    fn find_cursor(&self, lines: &[String]) -> Option<(usize, usize)> {
        for (i, line) in lines.iter().enumerate() {
            if let Some(pos) = line.find(CURSOR_MARKER) {
                let before = &line[..pos];
                let col = visible_width_simple(before);
                return Some((i, col));
            }
        }
        None
    }

    /// Position the hardware terminal cursor at the CURSOR_MARKER location.
    /// When `show_hardware_cursor` is enabled (e.g. for IME support) the
    /// blinking cursor is made visible; otherwise it is hidden.
    fn position_hardware_cursor(
        &mut self,
        cursor_pos: Option<(usize, usize)>,
        terminal: &mut dyn Terminal,
    ) {
        if let Some((row, col)) = cursor_pos {
            let current = self.hw_cursor_row;
            let mut buf = String::new();
            if row < current {
                buf.push_str(&format!("\x1b[{}A", current - row));
            } else if row > current {
                buf.push_str(&format!("\x1b[{}B", row - current));
            }
            buf.push_str(&format!("\r\x1b[{}C", col));
            if self.show_hardware_cursor {
                buf.push_str("\x1b[?25h"); // show cursor for IME
            } else {
                buf.push_str("\x1b[?25l"); // keep cursor hidden
            }
            terminal.write(&buf);
            self.hw_cursor_row = row;
        } else {
            terminal.write("\x1b[?25l"); // hide cursor
        }
    }

    fn save_state(&mut self, lines: &[String], width: u16, height: u16) {
        self.prev_lines = lines.to_vec();
        self.prev_width = width;
        self.prev_height = height;
    }
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}

fn visible_width_simple(s: &str) -> usize {
    let mut width = 0;
    let mut in_escape = false;
    let mut in_osc = false;
    for ch in s.chars() {
        if in_osc {
            if ch == '\x07' || ch == '\\' {
                in_osc = false;
            }
            continue;
        }
        if in_escape {
            if ch == ']' {
                in_osc = true;
                in_escape = false;
            } else if ch.is_ascii_alphabetic() || ch == '~' {
                in_escape = false;
            }
            continue;
        }
        if ch == '\x1b' {
            in_escape = true;
            continue;
        }
        if ch == '\x07' {
            continue;
        }
        width += unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
    }
    width
}
