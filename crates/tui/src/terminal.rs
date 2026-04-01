//! Terminal abstraction — matches pi-tui's terminal.ts.
//!
//! ProcessTerminal wraps crossterm to provide:
//! - Raw mode enable/disable
//! - Bracketed paste mode
//! - Synchronized output support
//! - Input and resize callbacks via crossterm event polling
//! - Column/row queries

use crossterm::{
    cursor, execute,
    event::{self, Event, KeyEvent, KeyCode, KeyModifiers},
    terminal,
};
use std::io::{self, Write};
use tokio::sync::mpsc;

/// Events from the terminal to the TUI event loop.
#[derive(Debug, Clone)]
pub enum TerminalEvent {
    /// A key was pressed.
    Key(KeyEvent),
    /// Bracketed paste content.
    Paste(String),
    /// Terminal was resized.
    Resize(u16, u16),
    /// A raw string (for sequences we can't parse as KeyEvent).
    Raw(String),
}

/// Abstraction over a terminal for rendering.
pub trait Terminal: Send {
    /// Enable raw mode, bracketed paste.
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

    /// Spawn a background task that polls crossterm events and sends them
    /// to the returned channel. Call this after `start()`.
    pub fn spawn_event_reader(&self) -> mpsc::UnboundedReceiver<TerminalEvent> {
        let (tx, rx) = mpsc::unbounded_channel();
        std::thread::spawn(move || {
            loop {
                match event::read() {
                    Ok(Event::Key(key)) => {
                        if tx.send(TerminalEvent::Key(key)).is_err() {
                            break;
                        }
                    }
                    Ok(Event::Paste(text)) => {
                        if tx.send(TerminalEvent::Paste(text)).is_err() {
                            break;
                        }
                    }
                    Ok(Event::Resize(w, h)) => {
                        if tx.send(TerminalEvent::Resize(w, h)).is_err() {
                            break;
                        }
                    }
                    Ok(_) => {} // mouse, focus, etc.
                    Err(_) => break,
                }
            }
        });
        rx
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
        // Show cursor
        execute!(stdout, cursor::Show).ok();
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
