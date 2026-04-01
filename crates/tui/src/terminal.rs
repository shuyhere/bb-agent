//! Terminal abstraction — matches pi-tui's terminal.ts.
//!
//! ProcessTerminal wraps crossterm to provide:
//! - Raw mode enable/disable
//! - Bracketed paste mode
//! - Synchronized output support
//! - Kitty keyboard protocol enable/disable with fallback
//! - Terminal title setting
//! - Input and resize callbacks via crossterm event polling
//! - Column/row queries

use crossterm::{
    cursor, execute,
    event::{self, Event, KeyEvent},
    terminal,
};
use std::io::{self, Write};
use tokio::sync::mpsc;

/// Synchronized output escape sequences (prevents flicker).
const SYNC_BEGIN: &str = "\x1b[?2026h";
const SYNC_END: &str = "\x1b[?2026l";

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
    /// Set terminal window title (OSC 0).
    fn set_title(&mut self, title: &str);
    /// Enable Kitty keyboard protocol (CSI >1u). Returns false if not supported.
    fn enable_kitty_protocol(&mut self) -> bool;
    /// Disable Kitty keyboard protocol (CSI <u).
    fn disable_kitty_protocol(&mut self);
    /// Whether the Kitty keyboard protocol is currently active.
    fn kitty_protocol_active(&self) -> bool;
    /// Begin synchronized output (CSI ?2026h) — prevents flicker.
    fn sync_begin(&mut self);
    /// End synchronized output (CSI ?2026l).
    fn sync_end(&mut self);
    /// Clear from cursor to end of screen.
    fn clear_from_cursor(&mut self);
    /// Clear entire screen and move cursor to home.
    fn clear_screen(&mut self);
    /// Clear current line.
    fn clear_line(&mut self);
}

/// A Terminal backed by the process's stdout.
pub struct ProcessTerminal {
    started: bool,
    kitty_active: bool,
}

impl ProcessTerminal {
    pub fn new() -> Self {
        Self {
            started: false,
            kitty_active: false,
        }
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
        // Disable Kitty protocol if active
        if self.kitty_active {
            self.disable_kitty_protocol();
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

    fn set_title(&mut self, title: &str) {
        // OSC 0;title BEL — set terminal window title
        let mut stdout = io::stdout();
        write!(stdout, "\x1b]0;{title}\x07").ok();
        stdout.flush().ok();
    }

    fn enable_kitty_protocol(&mut self) -> bool {
        if self.kitty_active {
            return true;
        }
        let mut stdout = io::stdout();
        // Push Kitty keyboard flags: CSI > 1 u
        // Flag 1 = disambiguate escape codes
        write!(stdout, "\x1b[>1u").ok();
        stdout.flush().ok();
        self.kitty_active = true;
        true
    }

    fn disable_kitty_protocol(&mut self) {
        if !self.kitty_active {
            return;
        }
        let mut stdout = io::stdout();
        // Pop Kitty keyboard flags: CSI < u
        write!(stdout, "\x1b[<u").ok();
        stdout.flush().ok();
        self.kitty_active = false;
    }

    fn kitty_protocol_active(&self) -> bool {
        self.kitty_active
    }

    fn sync_begin(&mut self) {
        let mut stdout = io::stdout();
        write!(stdout, "{SYNC_BEGIN}").ok();
        stdout.flush().ok();
    }

    fn sync_end(&mut self) {
        let mut stdout = io::stdout();
        write!(stdout, "{SYNC_END}").ok();
        stdout.flush().ok();
    }

    fn clear_from_cursor(&mut self) {
        let mut stdout = io::stdout();
        write!(stdout, "\x1b[J").ok();
        stdout.flush().ok();
    }

    fn clear_screen(&mut self) {
        let mut stdout = io::stdout();
        // Clear screen + move to home (1,1)
        write!(stdout, "\x1b[2J\x1b[H").ok();
        stdout.flush().ok();
    }

    fn clear_line(&mut self) {
        let mut stdout = io::stdout();
        write!(stdout, "\x1b[2K").ok();
        stdout.flush().ok();
    }
}

impl Drop for ProcessTerminal {
    fn drop(&mut self) {
        self.stop();
    }
}
