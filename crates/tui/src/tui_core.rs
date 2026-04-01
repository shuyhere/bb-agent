//! TUI — Main class managing the component tree and differential rendering.
//!
//! Matches pi-tui's TUI class: owns a Terminal and Renderer,
//! renders a component tree, manages focus, and handles input dispatch.

use crate::component::{Component, Container};
use crate::renderer::Renderer;
use crate::terminal::{ProcessTerminal, Terminal, TerminalEvent};
use crossterm::event::KeyEvent;
use tokio::sync::mpsc;

/// An overlay entry on the overlay stack.
pub struct OverlayEntry {
    /// The overlay component.
    pub component: Box<dyn Component>,
    /// The focus index that was active before this overlay was shown.
    pub pre_focus: Option<usize>,
    /// Whether this overlay is temporarily hidden.
    pub hidden: bool,
}

/// The main TUI engine. Holds the component tree and renders differentially.
pub struct TUI {
    pub terminal: ProcessTerminal,
    renderer: Renderer,
    /// The root container holding all components.
    pub root: Container,
    /// Index into root.children that currently has focus (receives input).
    focus_index: Option<usize>,
    stopped: bool,
    /// Stack of overlay components composited on top of base content.
    overlay_stack: Vec<OverlayEntry>,
}

impl TUI {
    pub fn new() -> Self {
        Self {
            terminal: ProcessTerminal::new(),
            renderer: Renderer::new(),
            root: Container::new(),
            focus_index: None,
            stopped: false,
            overlay_stack: Vec::new(),
        }
    }

    /// Start the terminal (raw mode, bracketed paste, hide cursor).
    pub fn start(&mut self) -> mpsc::UnboundedReceiver<TerminalEvent> {
        self.stopped = false;
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

    /// Render the component tree to the terminal, compositing visible overlays on top.
    pub fn render(&mut self) {
        if self.stopped {
            return;
        }
        let width = self.terminal.columns();
        let mut lines = self.root.render(width);

        // Composite visible overlays on top (bottom-anchored)
        for entry in &self.overlay_stack {
            if entry.hidden {
                continue;
            }
            let overlay_lines = entry.component.render(width);
            let overlay_len = overlay_lines.len();
            if overlay_len == 0 {
                continue;
            }
            // Ensure base has enough lines for overlay to replace
            while lines.len() < overlay_len {
                lines.push(String::new());
            }
            // Replace the bottom N lines of output with overlay lines
            let base_len = lines.len();
            let start = base_len - overlay_len;
            for (i, overlay_line) in overlay_lines.into_iter().enumerate() {
                lines[start + i] = overlay_line;
            }
        }

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

    /// Show an overlay component, pushing it onto the overlay stack.
    /// Focus switches to the overlay. Returns the overlay's handle ID (stack index at push time).
    pub fn show_overlay(&mut self, component: Box<dyn Component>) -> usize {
        let pre_focus = self.focus_index;
        let id = self.overlay_stack.len();
        self.overlay_stack.push(OverlayEntry {
            component,
            pre_focus,
            hidden: false,
        });
        // Focus is now on the overlay (set focus_index to None to indicate overlay has focus)
        self.focus_index = None;
        id
    }

    /// Hide (pop) the topmost overlay and restore focus to the previous target.
    pub fn hide_overlay(&mut self) {
        if let Some(entry) = self.overlay_stack.pop() {
            self.focus_index = entry.pre_focus;
        }
    }

    /// Check if there are any visible (non-hidden) overlays.
    pub fn has_overlay(&self) -> bool {
        self.overlay_stack.iter().any(|e| !e.hidden)
    }

    /// Temporarily hide or show an overlay by its handle ID.
    pub fn set_overlay_hidden(&mut self, id: usize, hidden: bool) {
        if let Some(entry) = self.overlay_stack.get_mut(id) {
            if entry.hidden == hidden {
                return;
            }
            entry.hidden = hidden;
            if hidden {
                // Restore focus to pre_focus when hiding
                self.focus_index = entry.pre_focus;
            } else {
                // When showing, capture focus (set to None to indicate overlay has focus)
                self.focus_index = None;
            }
        }
    }

    /// Dispatch a key event to the focused component (overlay or root child).
    pub fn handle_key(&mut self, key: &KeyEvent) {
        // If there's a visible overlay on top, send input to it
        if let Some(entry) = self.topmost_visible_overlay_mut() {
            entry.component.handle_input(key);
            return;
        }
        // Otherwise dispatch to focused root child
        if let Some(idx) = self.focus_index {
            if let Some(child) = self.root.children.get_mut(idx) {
                child.handle_input(key);
            }
        }
    }

    /// Dispatch raw input (e.g. paste) to the focused component.
    pub fn handle_raw_input(&mut self, data: &str) {
        // If there's a visible overlay on top, send input to it
        if let Some(entry) = self.topmost_visible_overlay_mut() {
            entry.component.handle_raw_input(data);
            return;
        }
        if let Some(idx) = self.focus_index {
            if let Some(child) = self.root.children.get_mut(idx) {
                child.handle_raw_input(data);
            }
        }
    }

    /// Get a mutable reference to the topmost visible overlay, if any.
    fn topmost_visible_overlay_mut(&mut self) -> Option<&mut OverlayEntry> {
        self.overlay_stack.iter_mut().rev().find(|e| !e.hidden)
    }

    /// Downcast the topmost visible overlay to a concrete component type.
    pub fn topmost_overlay_as_mut<T: 'static>(&mut self) -> Option<&mut T> {
        self.topmost_visible_overlay_mut()
            .and_then(|entry| entry.component.as_any_mut().downcast_mut::<T>())
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
