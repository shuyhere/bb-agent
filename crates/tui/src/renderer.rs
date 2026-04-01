use crate::terminal::Terminal;
use crate::utils::pad_to_width;

/// Synchronized output begin escape sequence.
const SYNC_BEGIN: &str = "\x1b[?2026h";
/// Synchronized output end escape sequence.
const SYNC_END: &str = "\x1b[?2026l";

/// A differential renderer that only redraws changed lines.
pub struct DiffRenderer {
    previous_lines: Vec<String>,
    previous_width: u16,
}

impl DiffRenderer {
    pub fn new() -> Self {
        Self {
            previous_lines: Vec::new(),
            previous_width: 0,
        }
    }

    /// Render the given lines to the terminal, updating only what changed.
    pub fn render(&mut self, new_lines: &[String], terminal: &mut dyn Terminal) {
        let width = terminal.columns();

        // Begin synchronized output
        terminal.write(SYNC_BEGIN);
        terminal.hide_cursor();

        if self.previous_lines.is_empty() || self.previous_width != width {
            // First render or width changed: clear and output everything
            // Move to top-left and clear screen
            terminal.write("\x1b[H\x1b[2J");
            for (i, line) in new_lines.iter().enumerate() {
                let padded = pad_to_width(line, width as usize);
                if i > 0 {
                    terminal.write("\r\n");
                }
                terminal.write(&padded);
            }
        } else {
            // Find first differing line
            let first_diff = self
                .previous_lines
                .iter()
                .zip(new_lines.iter())
                .position(|(old, new)| old != new)
                .unwrap_or_else(|| {
                    // All common lines match; diff starts at end of shorter
                    self.previous_lines.len().min(new_lines.len())
                });

            let max_lines = self.previous_lines.len().max(new_lines.len());

            if first_diff < max_lines {
                // Move cursor to the first changed line (1-indexed row)
                terminal.write(&format!("\x1b[{};1H", first_diff + 1));

                // Re-render from first_diff onward
                for i in first_diff..new_lines.len() {
                    let padded = pad_to_width(&new_lines[i], width as usize);
                    if i > first_diff {
                        terminal.write("\r\n");
                    }
                    terminal.write(&padded);
                }

                // If new content is shorter, clear remaining old lines
                if new_lines.len() < self.previous_lines.len() {
                    for _ in new_lines.len()..self.previous_lines.len() {
                        terminal.write("\r\n");
                        terminal.write(&" ".repeat(width as usize));
                    }
                    // Also clear from cursor to end of screen
                    terminal.write("\x1b[J");
                }
            }
        }

        terminal.show_cursor();
        // End synchronized output
        terminal.write(SYNC_END);

        self.previous_lines = new_lines.to_vec();
        self.previous_width = width;
    }

    /// Force a full redraw on next render.
    pub fn invalidate(&mut self) {
        self.previous_lines.clear();
    }
}

impl Default for DiffRenderer {
    fn default() -> Self {
        Self::new()
    }
}
