//! Scrollback-based differential renderer.
//!
//! Matches pi-tui's rendering approach:
//! - Content is written to the terminal's scrollback buffer (not fullscreen)
//! - Only changed lines are redrawn by moving cursor up to the first change
//! - All output wrapped in synchronized output (CSI ?2026h/l)
//! - Cursor positioned for IME via CURSOR_MARKER

use crate::component::CURSOR_MARKER;
use crate::terminal::Terminal;

/// Synchronized output escape sequences (prevents flicker).
const SYNC_BEGIN: &str = "\x1b[?2026h";
const SYNC_END: &str = "\x1b[?2026l";
/// Reset all styling.
const SEGMENT_RESET: &str = "\x1b[0m\x1b]8;;\x07";

pub struct Renderer {
    /// Lines previously rendered to the terminal.
    prev_lines: Vec<String>,
    /// Whether we've done the first render.
    first_render: bool,
}

impl Renderer {
    pub fn new() -> Self {
        Self {
            prev_lines: Vec::new(),
            first_render: true,
        }
    }

    /// Force next render to treat everything as new.
    pub fn invalidate(&mut self) {
        self.prev_lines.clear();
    }

    /// Render new lines to terminal, updating only what changed.
    pub fn render(&mut self, new_lines: &[String], terminal: &mut dyn Terminal) {
        // Apply line resets
        let new_lines: Vec<String> = new_lines
            .iter()
            .map(|l| format!("{l}{SEGMENT_RESET}"))
            .collect();

        // Find cursor marker position
        let mut cursor_line: Option<usize> = None;
        let mut cursor_col: Option<usize> = None;
        for (i, line) in new_lines.iter().enumerate() {
            if let Some(pos) = line.find(CURSOR_MARKER) {
                cursor_line = Some(i);
                // Count visible chars before the marker
                let before = &line[..pos];
                cursor_col = Some(visible_width_simple(before));
                break;
            }
        }

        // Strip CURSOR_MARKER from lines
        let new_lines: Vec<String> = new_lines
            .iter()
            .map(|l| l.replace(CURSOR_MARKER, ""))
            .collect();

        let prev_count = self.prev_lines.len();
        let new_count = new_lines.len();

        let mut buf = String::new();
        buf.push_str(SYNC_BEGIN);

        if self.first_render {
            // First render: just write all lines
            for (i, line) in new_lines.iter().enumerate() {
                if i > 0 {
                    buf.push_str("\r\n");
                }
                buf.push_str(line);
            }
            self.first_render = false;
        } else {
            // Subsequent renders: move cursor to start of our working area,
            // clear it, and rewrite all lines.
            // Our working area is prev_count lines up from current cursor position.
            if prev_count > 0 {
                // Move to start of working area
                let up = prev_count.saturating_sub(1);
                if up > 0 {
                    buf.push_str(&format!("\x1b[{}A", up));
                }
                buf.push('\r');
            }

            // Clear old working area and write new content
            let total_lines = prev_count.max(new_count);
            for i in 0..total_lines {
                if i > 0 {
                    buf.push_str("\r\n");
                }
                buf.push_str("\x1b[2K"); // clear line
                if i < new_count {
                    buf.push_str(&new_lines[i]);
                }
            }

            // If new content is shorter, cursor is now past the end.
            // Move it back up to the last line of new content.
            if prev_count > new_count && new_count > 0 {
                let extra = prev_count - new_count;
                if extra > 0 {
                    buf.push_str(&format!("\x1b[{}A", extra));
                }
            }
        }
        // else: no changes at all, do nothing

        // Position hardware cursor for IME
        if let (Some(cl), Some(cc)) = (cursor_line, cursor_col) {
            // Cursor is at the last rendered line (bottom of our content)
            // We need to move it to the cursor line
            let current_line = new_count.saturating_sub(1);
            let move_up = current_line.saturating_sub(cl);
            if move_up > 0 {
                buf.push_str(&format!("\x1b[{}A", move_up));
            }
            buf.push_str(&format!("\r\x1b[{}C", cc));
            // Show cursor
            buf.push_str("\x1b[?25h");
        } else {
            // Hide cursor when no marker
            buf.push_str("\x1b[?25l");
        }

        buf.push_str(SYNC_END);
        terminal.write(&buf);

        self.prev_lines = new_lines;
    }
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}

/// Simple visible width calculation (counts non-ANSI characters).
fn visible_width_simple(s: &str) -> usize {
    let mut width = 0;
    let mut in_escape = false;
    for ch in s.chars() {
        if in_escape {
            if ch.is_ascii_alphabetic() || ch == '~' {
                in_escape = false;
            }
            continue;
        }
        if ch == '\x1b' {
            in_escape = true;
            continue;
        }
        if ch == '\x07' {
            continue; // BEL
        }
        width += unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
    }
    width
}
