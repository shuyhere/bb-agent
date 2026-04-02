//! TUI — Main class managing the component tree and differential rendering.
//!
//! Matches pi-tui's TUI class: owns a Terminal and Renderer,
//! renders a component tree, manages focus, and handles input dispatch.

use crate::component::{Component, Container};
use crate::renderer::Renderer;
use crate::terminal::{ProcessTerminal, Terminal, TerminalEvent};
use crossterm::event::KeyEvent;
use tokio::sync::mpsc;

/// Result of an input listener intercepting input.
pub enum InputResult {
    /// Input was consumed; stop propagation.
    Consumed,
    /// Input was transformed; continue with the new value.
    Transformed(String),
    /// Input was not handled; continue with original value.
    Ignored,
}

/// How an overlay is anchored on screen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OverlayAnchor {
    /// Overlay replaces the bottom N lines of base content (current behaviour).
    #[default]
    Bottom,
    /// Overlay is centred vertically in the terminal.
    Center,
}

/// Options controlling overlay presentation.
#[derive(Clone, Debug)]
pub struct OverlayOptions {
    pub anchor: OverlayAnchor,
}

impl Default for OverlayOptions {
    fn default() -> Self {
        Self {
            anchor: OverlayAnchor::Bottom,
        }
    }
}

/// An overlay entry on the overlay stack.
pub struct OverlayEntry {
    /// The overlay component.
    pub component: Box<dyn Component>,
    /// The focus index that was active before this overlay was shown.
    pub pre_focus: Option<usize>,
    /// Whether this overlay is temporarily hidden.
    pub hidden: bool,
    /// Positioning options for this overlay.
    pub options: OverlayOptions,
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
    /// Render-batching flag: set by `request_render()`, cleared by `flush_render()`.
    render_requested: bool,
    /// Input middleware — listeners run before input reaches the focused component.
    input_listeners: Vec<Box<dyn Fn(&str) -> InputResult + Send>>,
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
            render_requested: false,
            input_listeners: Vec::new(),
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

    /// Mark that a render is needed. Call `flush_render()` to actually render.
    /// This prevents redundant renders when multiple state changes occur in a
    /// single event handler.
    pub fn request_render(&mut self) {
        self.render_requested = true;
    }

    /// If a render was requested, perform it and clear the flag.
    pub fn flush_render(&mut self) {
        if self.render_requested {
            self.render_requested = false;
            self.render();
        }
    }

    /// Render the component tree to the terminal, compositing visible overlays on top.
    pub fn render(&mut self) {
        if self.stopped {
            return;
        }
        self.render_requested = false;
        let width = self.terminal.columns();
        let height = self.terminal.rows();
        let mut lines = self.root.render(width);

        // Composite visible overlays on top
        for entry in &self.overlay_stack {
            if entry.hidden {
                continue;
            }
            let overlay_lines = entry.component.render(width);
            let overlay_len = overlay_lines.len();
            if overlay_len == 0 {
                continue;
            }

            match entry.options.anchor {
                OverlayAnchor::Bottom => {
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
                OverlayAnchor::Center => {
                    let h = height as usize;
                    // Ensure base has at least `h` lines so centering maths work
                    while lines.len() < h {
                        lines.push(String::new());
                    }
                    let start = if h > overlay_len {
                        (h - overlay_len) / 2
                    } else {
                        0
                    };
                    for (i, overlay_line) in overlay_lines.into_iter().enumerate() {
                        let idx = start + i;
                        if idx < lines.len() {
                            lines[idx] = overlay_line;
                        }
                    }
                }
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

    /// Show an overlay component with default (Bottom) anchor.
    /// Focus switches to the overlay. Returns the overlay's handle ID.
    pub fn show_overlay(&mut self, component: Box<dyn Component>) -> usize {
        self.show_overlay_with(component, OverlayOptions::default())
    }

    /// Show an overlay component with explicit positioning options.
    /// Focus switches to the overlay. Returns the overlay's handle ID (stack index at push time).
    pub fn show_overlay_with(
        &mut self,
        component: Box<dyn Component>,
        options: OverlayOptions,
    ) -> usize {
        let pre_focus = self.focus_index;
        let id = self.overlay_stack.len();
        self.overlay_stack.push(OverlayEntry {
            component,
            pre_focus,
            hidden: false,
            options,
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

    /// Add an input listener that can intercept/transform input before it
    /// reaches the focused component.
    pub fn add_input_listener(&mut self, listener: Box<dyn Fn(&str) -> InputResult + Send>) {
        self.input_listeners.push(listener);
    }

    /// Dispatch a key event to the focused component (overlay or root child).
    /// Input listeners run first; if any returns `Consumed` dispatch is skipped.
    pub fn handle_key(&mut self, key: &KeyEvent) {
        // Run input listeners with a descriptive key string
        let key_str = format!("{key:?}");
        for listener in &self.input_listeners {
            match listener(&key_str) {
                InputResult::Consumed => return,
                InputResult::Transformed(_) | InputResult::Ignored => {}
            }
        }

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
    /// Input listeners run first and may consume or transform the input.
    pub fn handle_raw_input(&mut self, data: &str) {
        let mut current = data.to_string();
        for listener in &self.input_listeners {
            match listener(&current) {
                InputResult::Consumed => return,
                InputResult::Transformed(new) => current = new,
                InputResult::Ignored => {}
            }
        }

        // If there's a visible overlay on top, send input to it
        if let Some(entry) = self.topmost_visible_overlay_mut() {
            entry.component.handle_raw_input(&current);
            return;
        }
        if let Some(idx) = self.focus_index {
            if let Some(child) = self.root.children.get_mut(idx) {
                child.handle_raw_input(&current);
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
