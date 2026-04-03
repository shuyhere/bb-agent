use std::io;

use super::terminal::FullscreenTerminal;

#[derive(Clone, Debug, Default)]
pub struct FrameBuffer {
    pub lines: Vec<String>,
    pub cursor: Option<(u16, u16)>,
}

#[derive(Default)]
pub struct FullscreenRenderer {
    previous_lines: Vec<String>,
}

impl FullscreenRenderer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn render(
        &mut self,
        terminal: &mut FullscreenTerminal,
        frame: &FrameBuffer,
    ) -> io::Result<()> {
        let mut buf = String::new();
        terminal.begin_sync(&mut buf);
        buf.push_str("\x1b[?25l");

        let full_repaint = self.previous_lines.len() != frame.lines.len();
        for (row, line) in frame.lines.iter().enumerate() {
            let changed = full_repaint
                || self
                    .previous_lines
                    .get(row)
                    .map(|previous| previous != line)
                    .unwrap_or(true);
            if !changed {
                continue;
            }

            buf.push_str(&move_to(0, row as u16));
            buf.push_str("\x1b[2K");
            buf.push_str(line);
        }

        if self.previous_lines.len() > frame.lines.len() {
            for row in frame.lines.len()..self.previous_lines.len() {
                buf.push_str(&move_to(0, row as u16));
                buf.push_str("\x1b[2K");
            }
        }

        match frame.cursor {
            Some((x, y)) => {
                buf.push_str(&move_to(x, y));
                buf.push_str("\x1b[?25h");
            }
            None => {
                buf.push_str("\x1b[?25l");
            }
        }

        terminal.end_sync(&mut buf);
        terminal.write_raw(&buf)?;

        self.previous_lines = frame.lines.clone();
        Ok(())
    }
}

fn move_to(x: u16, y: u16) -> String {
    format!("\x1b[{};{}H", y + 1, x + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn move_to_uses_one_based_coordinates() {
        assert_eq!(move_to(0, 0), "\x1b[1;1H");
        assert_eq!(move_to(4, 2), "\x1b[3;5H");
    }
}
