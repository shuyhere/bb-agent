//! Scrollback-based differential renderer.
//!
//! Matches pi-tui's rendering approach:
//! - Content is appended to the terminal scrollback buffer (not fullscreen)
//! - Only changed lines are redrawn
//! - All output wrapped in synchronized output (CSI ?2026h/l)
//! - Cursor positioned at the end of content after render

use crate::terminal::Terminal;
use crate::utils::visible_width;

/// Synchronized output escape sequences (prevents flicker).
const SYNC_BEGIN: &str = "\x1b[?2026h";
const SYNC_END: &str = "\x1b[?2026l";
/// Reset all styling.
const RESET: &str = "\x1b[0m";

pub struct Renderer {
    /// Lines previously rendered to the terminal.
    prev_lines: Vec<String>,
    /// Previous terminal width.
    prev_width: u16,
    /// Current cursor row (0-indexed from first rendered line).
    cursor_row: usize,
    /// Whether we've done the first render.
    first_render: bool,
}

impl Renderer {
    pub fn new() -> Self {
        Self {
            prev_lines: Vec::new(),
            prev_width: 0,
            cursor_row: 0,
            first_render: true,
        }
    }

    /// Render new lines to terminal, updating only what changed.
    pub fn render(&mut self, new_lines: &[String], terminal: &mut dyn Terminal) {
        let width = terminal.columns();

        // Append reset to each line to prevent style bleed
        let new_lines: Vec<String> = new_lines
            .iter()
            .map(|l| format!("{l}{RESET}"))
            .collect();

        // Begin synchronized output
        let mut buf = String::new();
        buf.push_str(SYNC_BEGIN);

        if self.first_render {
            // First render: just output all lines
            for (i, line) in new_lines.iter().enumerate() {
                if i > 0 {
                    buf.push_str("\r\n");
                }
                buf.push_str(line);
            }
            self.cursor_row = new_lines.len().saturating_sub(1);
            self.first_render = false;
        } else if self.prev_width != width {
            // Width changed: clear and re-render everything
            // Move cursor to the start of our content
            self.move_to_first_line(&mut buf);
            // Clear from cursor to end of screen
            buf.push_str("\x1b[J");
            // Re-render all
            for (i, line) in new_lines.iter().enumerate() {
                if i > 0 {
                    buf.push_str("\r\n");
                }
                buf.push_str(line);
            }
            self.cursor_row = new_lines.len().saturating_sub(1);
        } else {
            // Normal update: find first changed line
            let first_changed = self.find_first_changed(&new_lines);

            if let Some(first) = first_changed {
                // Move cursor to the first changed line
                let diff = self.cursor_row as isize - first as isize;
                if diff > 0 {
                    buf.push_str(&format!("\x1b[{}A", diff));
                } else if diff < 0 {
                    buf.push_str(&format!("\x1b[{}B", -diff));
                }
                buf.push('\r');

                // Re-render from first changed to end of new content
                for i in first..new_lines.len() {
                    if i > first {
                        buf.push_str("\r\n");
                    }
                    buf.push_str("\x1b[2K"); // clear line
                    buf.push_str(&new_lines[i]);
                }

                // Clear any extra old lines
                if new_lines.len() < self.prev_lines.len() {
                    let extra = self.prev_lines.len() - new_lines.len();
                    for _ in 0..extra {
                        buf.push_str("\r\n\x1b[2K");
                    }
                    // Move back up
                    if extra > 0 {
                        buf.push_str(&format!("\x1b[{}A", extra));
                    }
                }

                self.cursor_row = new_lines.len().saturating_sub(1);
            }
            // else: no changes, nothing to do
        }

        buf.push_str(SYNC_END);
        terminal.write(&buf);

        self.prev_lines = new_lines;
        self.prev_width = width;
    }

    /// Force full re-render on next call.
    pub fn invalidate(&mut self) {
        self.prev_lines.clear();
        self.first_render = true;
        self.cursor_row = 0;
    }

    /// Move cursor to the first line of our content.
    fn move_to_first_line(&self, buf: &mut String) {
        if self.cursor_row > 0 {
            buf.push_str(&format!("\x1b[{}A", self.cursor_row));
        }
        buf.push('\r');
    }

    /// Find the first line that differs between prev and new.
    fn find_first_changed(&self, new_lines: &[String]) -> Option<usize> {
        let max = self.prev_lines.len().max(new_lines.len());
        for i in 0..max {
            let old = self.prev_lines.get(i).map(|s| s.as_str()).unwrap_or("");
            let new = new_lines.get(i).map(|s| s.as_str()).unwrap_or("");
            if old != new {
                return Some(i);
            }
        }
        None
    }
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}
