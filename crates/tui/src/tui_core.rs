//! TUI — Main class managing the component tree and differential rendering.
//!
//! Matches pi-tui's TUI class: owns a Terminal and Renderer,
//! renders a component tree, manages focus, and handles input dispatch.

use crate::component::{Component, Container};
use crate::renderer::Renderer;
use crate::terminal::{ProcessTerminal, Terminal, TerminalEvent};
use crossterm::event::KeyEvent;
use tokio::sync::mpsc;

/// The main TUI engine. Holds the component tree and renders differentially.
pub struct TUI {
    pub terminal: ProcessTerminal,
    renderer: Renderer,
    /// The root container holding all components.
    pub root: Container,
    /// Index into root.children that currently has focus (receives input).
    focus_index: Option<usize>,
    stopped: bool,
}

impl TUI {
    pub fn new() -> Self {
        Self {
            terminal: ProcessTerminal::new(),
            renderer: Renderer::new(),
            root: Container::new(),
            focus_index: None,
            stopped: false,
        }
    }

    /// Start the terminal (raw mode, bracketed paste, hide cursor).
    pub fn start(&mut self) -> mpsc::UnboundedReceiver<TerminalEvent> {
        self.terminal.start();
        self.terminal.hide_cursor();
        let rx = self.terminal.spawn_event_reader();
        rx
    }

    /// Stop the terminal and restore state.
    pub fn stop(&mut self) {
        if self.stopped {
            return;
        }
        self.stopped = true;

        // Move cursor past rendered content
        let mut buf = String::new();
        buf.push_str("\r\n");
        self.terminal.write(&buf);
        self.terminal.show_cursor();
        self.terminal.stop();
    }

    /// Render the component tree to the terminal.
    pub fn render(&mut self) {
        if self.stopped {
            return;
        }
        let width = self.terminal.columns();
        let lines = self.root.render(width);
        self.renderer.render(&lines, &mut self.terminal);
    }

    /// Force full re-render (e.g., after terminal resize).
    pub fn force_render(&mut self) {
        self.renderer.invalidate();
        self.render();
    }

    /// Set focus to a specific child index.
    pub fn set_focus(&mut self, index: Option<usize>) {
        self.focus_index = index;
    }

    /// Get the currently focused child index.
    pub fn focus_index(&self) -> Option<usize> {
        self.focus_index
    }

    /// Dispatch a key event to the focused component.
    pub fn handle_key(&mut self, key: &KeyEvent) {
        if let Some(idx) = self.focus_index {
            if let Some(child) = self.root.children.get_mut(idx) {
                child.handle_input(key);
            }
        }
    }

    /// Dispatch raw input (e.g. paste) to the focused component.
    pub fn handle_raw_input(&mut self, data: &str) {
        if let Some(idx) = self.focus_index {
            if let Some(child) = self.root.children.get_mut(idx) {
                child.handle_raw_input(data);
            }
        }
    }

    /// Terminal width.
    pub fn columns(&self) -> u16 {
        self.terminal.columns()
    }

    /// Terminal height.
    pub fn rows(&self) -> u16 {
        self.terminal.rows()
    }
}

impl Default for TUI {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for TUI {
    fn drop(&mut self) {
        self.stop();
    }
}
