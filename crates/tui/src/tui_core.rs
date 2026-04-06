//! TUI — Main class managing the component tree and differential rendering.
//!
//! Matches the earlier TypeScript TUI class: owns a Terminal and Renderer,
//! renders a component tree, manages focus, and handles input dispatch.

mod focus;
mod input;
mod layout;
mod overlay;
mod rendering;

#[cfg(test)]
mod tests;

use crate::component::{Component, Container};
use crate::renderer::Renderer;
use crate::terminal::{ProcessTerminal, Terminal, TerminalEvent};
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
    /// Legacy: full-width, bottom of content (current behaviour).
    Bottom,
    /// Centred vertically and horizontally.
    #[default]
    Center,
    TopLeft,
    TopCenter,
    TopRight,
    BottomLeft,
    BottomCenter,
    BottomRight,
    LeftCenter,
    RightCenter,
}

/// A size that is either an absolute number of cells or a percentage.
#[derive(Clone, Debug)]
pub enum SizeValue {
    Absolute(usize),
    Percent(f32),
}

impl SizeValue {
    /// Resolve to an absolute value given a reference size.
    pub fn resolve(&self, reference: usize) -> usize {
        match self {
            SizeValue::Absolute(value) => *value,
            SizeValue::Percent(percent) => ((reference as f32) * percent / 100.0).floor() as usize,
        }
    }
}

/// Margin from terminal edges for overlay positioning.
#[derive(Clone, Debug, Default)]
pub struct OverlayMargin {
    pub top: usize,
    pub right: usize,
    pub bottom: usize,
    pub left: usize,
}

impl OverlayMargin {
    /// Create a uniform margin on all sides.
    pub fn uniform(value: usize) -> Self {
        Self {
            top: value,
            right: value,
            bottom: value,
            left: value,
        }
    }
}

/// Options controlling overlay presentation.
#[derive(Clone, Debug, Default)]
pub struct OverlayOptions {
    pub width: Option<SizeValue>,
    pub min_width: Option<usize>,
    pub max_height: Option<SizeValue>,
    pub anchor: OverlayAnchor,
    pub offset_x: i32,
    pub offset_y: i32,
    pub row: Option<SizeValue>,
    pub col: Option<SizeValue>,
    pub margin: OverlayMargin,
    pub non_capturing: bool,
}

/// Resolved overlay layout computed from OverlayOptions + terminal dimensions.
pub(crate) struct ResolvedLayout {
    pub(crate) width: usize,
    pub(crate) row: usize,
    pub(crate) col: usize,
    pub(crate) max_height: Option<usize>,
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
    /// If true, this overlay doesn't steal focus.
    pub non_capturing: bool,
}

/// Detect Termux session. In Termux the software keyboard toggling changes
/// terminal height; we skip full redraws on height change to avoid flicker.
pub fn is_termux() -> bool {
    std::env::var("TERMUX_VERSION").is_ok()
}

/// ANSI reset + hyperlink close, used between composited segments.
const SEGMENT_RESET: &str = "\x1b[0m\x1b]8;;\x07";

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
    input_listeners: Vec<InputListener>,
    /// Global callback for debug key (Shift+Ctrl+D).
    pub on_debug: Option<Box<dyn FnMut() + Send>>,
}

type InputListener = Box<dyn Fn(&str) -> InputResult + Send>;

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
            on_debug: None,
        }
    }

    /// Start the terminal (raw mode, bracketed paste, hide cursor).
    pub fn start(&mut self) -> mpsc::UnboundedReceiver<TerminalEvent> {
        self.stopped = false;
        self.terminal.start();
        self.terminal.hide_cursor();
        self.terminal.spawn_event_reader()
    }

    /// Stop the terminal and restore state.
    pub fn stop(&mut self) {
        if self.stopped {
            return;
        }
        self.stopped = true;

        let mut buf = String::new();
        buf.push_str("\r\n");
        self.terminal.write(&buf);
        self.terminal.show_cursor();
        self.terminal.stop();
    }

    /// Mark that a render is needed. Call `flush_render()` to actually render.
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

    /// Mark root content as changed (no-op — kept for API compat).
    pub fn invalidate_root(&mut self) {
        // Root is always re-rendered from components. The differential
        // renderer handles skipping unchanged lines efficiently.
    }

    /// Force full re-render (e.g., after terminal resize).
    pub fn force_render(&mut self) {
        self.renderer.invalidate();
        self.render();
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
