use crossterm::{cursor, execute, terminal};
use std::io::{self, Write};

/// Abstraction over a terminal for rendering.
pub trait Terminal {
    /// Enable raw mode, bracketed paste, alternate screen prep.
    fn start(&mut self);
    /// Restore terminal state.
    fn stop(&mut self);
    /// Write raw data to the terminal.
    fn write(&mut self, data: &str);
    /// Terminal width in columns.
    fn columns(&self) -> u16;
    /// Terminal height in rows.
    fn rows(&self) -> u16;
    /// Hide the cursor.
    fn hide_cursor(&mut self);
    /// Show the cursor.
    fn show_cursor(&mut self);
}

/// A Terminal backed by the process's stdout.
pub struct ProcessTerminal {
    started: bool,
}

impl ProcessTerminal {
    pub fn new() -> Self {
        Self { started: false }
    }
}

impl Default for ProcessTerminal {
    fn default() -> Self {
        Self::new()
    }
}

impl Terminal for ProcessTerminal {
    fn start(&mut self) {
        if self.started {
            return;
        }
        terminal::enable_raw_mode().ok();
        let mut stdout = io::stdout();
        // Enable bracketed paste mode
        write!(stdout, "\x1b[?2004h").ok();
        stdout.flush().ok();
        self.started = true;
    }

    fn stop(&mut self) {
        if !self.started {
            return;
        }
        let mut stdout = io::stdout();
        // Disable bracketed paste mode
        write!(stdout, "\x1b[?2004l").ok();
        stdout.flush().ok();
        terminal::disable_raw_mode().ok();
        self.started = false;
    }

    fn write(&mut self, data: &str) {
        let mut stdout = io::stdout();
        write!(stdout, "{data}").ok();
        stdout.flush().ok();
    }

    fn columns(&self) -> u16 {
        terminal::size().map(|(w, _)| w).unwrap_or(80)
    }

    fn rows(&self) -> u16 {
        terminal::size().map(|(_, h)| h).unwrap_or(24)
    }

    fn hide_cursor(&mut self) {
        let mut stdout = io::stdout();
        execute!(stdout, cursor::Hide).ok();
    }

    fn show_cursor(&mut self) {
        let mut stdout = io::stdout();
        execute!(stdout, cursor::Show).ok();
    }
}

impl Drop for ProcessTerminal {
    fn drop(&mut self) {
        self.stop();
    }
}
