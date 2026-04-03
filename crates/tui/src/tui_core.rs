//! TUI — Main class managing the component tree and differential rendering.
//!
//! Matches pi-tui's TUI class: owns a Terminal and Renderer,
//! renders a component tree, manages focus, and handles input dispatch.

use crate::component::{BOTTOM_ANCHOR_MARKER, Component, Container};
use crate::renderer::Renderer;
use crate::terminal::{ProcessTerminal, Terminal, TerminalEvent};
use crate::utils::{extract_segments, visible_width};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
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

// ---------------------------------------------------------------------------
// Overlay types
// ---------------------------------------------------------------------------

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
            SizeValue::Absolute(v) => *v,
            SizeValue::Percent(p) => ((reference as f32) * p / 100.0).floor() as usize,
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
    pub fn uniform(v: usize) -> Self {
        Self {
            top: v,
            right: v,
            bottom: v,
            left: v,
        }
    }
}

/// Options controlling overlay presentation.
#[derive(Clone, Debug, Default)]
pub struct OverlayOptions {
    // --- Sizing ---
    /// Width in columns, or percentage of terminal width.
    pub width: Option<SizeValue>,
    /// Minimum width in columns.
    pub min_width: Option<usize>,
    /// Maximum height in rows, or percentage of terminal height.
    pub max_height: Option<SizeValue>,

    // --- Positioning: anchor-based ---
    /// Anchor point for positioning (default: Bottom for legacy compat).
    pub anchor: OverlayAnchor,
    /// Horizontal offset from anchor position (positive = right).
    pub offset_x: i32,
    /// Vertical offset from anchor position (positive = down).
    pub offset_y: i32,

    // --- Positioning: absolute / percentage ---
    /// Row position: absolute or percentage.
    pub row: Option<SizeValue>,
    /// Column position: absolute or percentage.
    pub col: Option<SizeValue>,

    // --- Margin from terminal edges ---
    pub margin: OverlayMargin,

    // --- Behaviour ---
    /// If true, don't capture keyboard focus when shown.
    pub non_capturing: bool,
}

/// Resolved overlay layout computed from OverlayOptions + terminal dimensions.
struct ResolvedLayout {
    width: usize,
    row: usize,
    col: usize,
    max_height: Option<usize>,
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

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Detect Termux session. In Termux the software keyboard toggling changes
/// terminal height; we skip full redraws on height change to avoid flicker.
pub fn is_termux() -> bool {
    std::env::var("TERMUX_VERSION").is_ok()
}

/// ANSI reset + hyperlink close, used between composited segments.
const SEGMENT_RESET: &str = "\x1b[0m\x1b]8;;\x07";

// ---------------------------------------------------------------------------
// TUI
// ---------------------------------------------------------------------------

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
    /// Global callback for debug key (Shift+Ctrl+D).
    pub on_debug: Option<Box<dyn FnMut() + Send>>,
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
            on_debug: None,
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

    // -----------------------------------------------------------------------
    // Rendering
    // -----------------------------------------------------------------------

    /// Render the component tree to the terminal, compositing visible overlays on top.
    pub fn render(&mut self) {
        if self.stopped {
            return;
        }
        self.render_requested = false;
        let width = self.terminal.columns();
        let height = self.terminal.rows();
        let mut lines = self.root.render(width);
        lines = Self::apply_bottom_anchor(lines, height as usize);

        // Composite visible overlays
        if !self.overlay_stack.is_empty() {
            lines = self.composite_overlays(lines, width as usize, height as usize);
        }

        self.renderer.render(&lines, &mut self.terminal);
    }

    fn apply_bottom_anchor(lines: Vec<String>, term_height: usize) -> Vec<String> {
        let mut cleaned = Vec::with_capacity(lines.len());
        let mut anchor_idx: Option<usize> = None;

        for line in lines {
            if line.contains(BOTTOM_ANCHOR_MARKER) {
                anchor_idx = Some(cleaned.len());
                let stripped = line.replace(BOTTOM_ANCHOR_MARKER, "");
                if !stripped.is_empty() {
                    cleaned.push(stripped);
                }
            } else {
                cleaned.push(line);
            }
        }

        if let Some(anchor_idx) = anchor_idx {
            if cleaned.len() < term_height {
                let pad = term_height - cleaned.len();
                cleaned.splice(anchor_idx..anchor_idx, std::iter::repeat(String::new()).take(pad));
            }
        }

        cleaned
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

    // -----------------------------------------------------------------------
    // Overlay compositing (ported from pi's compositeOverlays / compositeLineAt)
    // -----------------------------------------------------------------------

    /// Composite all visible overlays into the base content lines.
    fn composite_overlays(
        &self,
        lines: Vec<String>,
        term_width: usize,
        term_height: usize,
    ) -> Vec<String> {
        let mut result = lines;

        // Collect visible entries (preserve insertion order).
        let visible: Vec<usize> = self
            .overlay_stack
            .iter()
            .enumerate()
            .filter(|(_, e)| !e.hidden)
            .map(|(i, _)| i)
            .collect();

        if visible.is_empty() {
            return result;
        }

        // Pre-render overlays and compute positions.
        struct Rendered {
            lines: Vec<String>,
            row: usize,
            col: usize,
            width: usize,
        }

        let mut rendered: Vec<Rendered> = Vec::new();
        let mut min_lines_needed = result.len();

        for &idx in &visible {
            let entry = &self.overlay_stack[idx];

            // Legacy Bottom anchor: full-width, bottom-of-content.
            if entry.options.anchor == OverlayAnchor::Bottom
                && entry.options.col.is_none()
                && entry.options.row.is_none()
                && entry.options.width.is_none()
            {
                let overlay_lines = entry.component.render(term_width as u16);
                let ol = overlay_lines.len();
                if ol == 0 {
                    continue;
                }
                while result.len() < ol {
                    result.push(String::new());
                }
                let start = result.len() - ol;
                for (i, line) in overlay_lines.into_iter().enumerate() {
                    result[start + i] = line;
                }
                continue;
            }

            // New positioned overlay system
            // First resolve layout without knowing overlay height (to get width/maxHeight).
            let layout0 = Self::resolve_overlay_layout(&entry.options, 0, term_width, term_height);
            let render_width = layout0.width.max(1) as u16;

            let mut overlay_lines = entry.component.render(render_width);
            if let Some(mh) = layout0.max_height {
                overlay_lines.truncate(mh);
            }
            if overlay_lines.is_empty() {
                continue;
            }

            // Re-resolve with actual overlay height.
            let layout =
                Self::resolve_overlay_layout(&entry.options, overlay_lines.len(), term_width, term_height);

            min_lines_needed = min_lines_needed.max(layout.row + overlay_lines.len());
            rendered.push(Rendered {
                lines: overlay_lines,
                row: layout.row,
                col: layout.col,
                width: layout.width,
            });
        }

        // Extend result to cover working area.
        let working_height = min_lines_needed.max(result.len());
        while result.len() < working_height {
            result.push(String::new());
        }

        let viewport_start = working_height.saturating_sub(term_height);

        for r in &rendered {
            for (i, overlay_line) in r.lines.iter().enumerate() {
                let idx = viewport_start + r.row + i;
                if idx < result.len() {
                    // Truncate overlay line to declared width before compositing.
                    let ol = if visible_width(overlay_line) > r.width {
                        crate::utils::truncate_to_width(overlay_line, r.width)
                    } else {
                        overlay_line.clone()
                    };
                    result[idx] =
                        Self::composite_line_at(&result[idx], &ol, r.col, r.width, term_width);
                }
            }
        }

        result
    }

    /// Splice overlay content into a base line at a specific column.
    fn composite_line_at(
        base_line: &str,
        overlay_line: &str,
        start_col: usize,
        overlay_width: usize,
        total_width: usize,
    ) -> String {
        let after_start = start_col + overlay_width;
        let after_width = total_width.saturating_sub(after_start);

        let seg = extract_segments(base_line, start_col, after_start, after_width);

        let before_pad = start_col.saturating_sub(seg.before_width);
        let overlay_vw = visible_width(overlay_line);
        let overlay_pad = overlay_width.saturating_sub(overlay_vw);

        let actual_before = start_col.max(seg.before_width);
        let actual_overlay = overlay_width.max(overlay_vw);
        let after_target = total_width.saturating_sub(actual_before + actual_overlay);
        let after_pad = after_target.saturating_sub(seg.after_width);

        let mut out = String::with_capacity(
            seg.before.len()
                + before_pad
                + SEGMENT_RESET.len()
                + overlay_line.len()
                + overlay_pad
                + SEGMENT_RESET.len()
                + seg.after.len()
                + after_pad,
        );
        out.push_str(&seg.before);
        for _ in 0..before_pad {
            out.push(' ');
        }
        out.push_str(SEGMENT_RESET);
        out.push_str(overlay_line);
        for _ in 0..overlay_pad {
            out.push(' ');
        }
        out.push_str(SEGMENT_RESET);
        out.push_str(&seg.after);
        for _ in 0..after_pad {
            out.push(' ');
        }

        // Safety: truncate to terminal width to prevent overflow.
        let result_vw = visible_width(&out);
        if result_vw > total_width {
            crate::utils::truncate_to_width(&out, total_width)
        } else {
            out
        }
    }

    /// Resolve overlay layout from options and terminal dimensions.
    fn resolve_overlay_layout(
        options: &OverlayOptions,
        overlay_height: usize,
        term_width: usize,
        term_height: usize,
    ) -> ResolvedLayout {
        let m = &options.margin;
        let margin_top = m.top;
        let margin_right = m.right;
        let margin_bottom = m.bottom;
        let margin_left = m.left;

        let avail_w = term_width.saturating_sub(margin_left + margin_right).max(1);
        let avail_h = term_height.saturating_sub(margin_top + margin_bottom).max(1);

        // --- Width ---
        let mut width = options
            .width
            .as_ref()
            .map(|sv| sv.resolve(term_width))
            .unwrap_or_else(|| avail_w.min(80));
        if let Some(mw) = options.min_width {
            width = width.max(mw);
        }
        width = width.clamp(1, avail_w);

        // --- maxHeight ---
        let max_height = options.max_height.as_ref().map(|sv| {
            let mh = sv.resolve(term_height);
            mh.clamp(1, avail_h)
        });

        let eff_height = max_height
            .map(|mh| overlay_height.min(mh))
            .unwrap_or(overlay_height);

        // --- Row ---
        let row = if let Some(ref sv) = options.row {
            match sv {
                SizeValue::Percent(p) => {
                    let max_row = avail_h.saturating_sub(eff_height);
                    margin_top + ((max_row as f32 * p / 100.0).floor() as usize)
                }
                SizeValue::Absolute(v) => *v,
            }
        } else {
            Self::resolve_anchor_row(options.anchor, eff_height, avail_h, margin_top)
        };

        // --- Col ---
        let col = if let Some(ref sv) = options.col {
            match sv {
                SizeValue::Percent(p) => {
                    let max_col = avail_w.saturating_sub(width);
                    margin_left + ((max_col as f32 * p / 100.0).floor() as usize)
                }
                SizeValue::Absolute(v) => *v,
            }
        } else {
            Self::resolve_anchor_col(options.anchor, width, avail_w, margin_left)
        };

        // Apply offsets.
        let row = (row as i32 + options.offset_y).max(0) as usize;
        let col = (col as i32 + options.offset_x).max(0) as usize;

        // Clamp to terminal bounds (respecting margins).
        let row = row.clamp(margin_top, term_height.saturating_sub(margin_bottom + eff_height));
        let col = col.clamp(margin_left, term_width.saturating_sub(margin_right + width));

        ResolvedLayout {
            width,
            row,
            col,
            max_height,
        }
    }

    fn resolve_anchor_row(
        anchor: OverlayAnchor,
        height: usize,
        avail_h: usize,
        margin_top: usize,
    ) -> usize {
        match anchor {
            OverlayAnchor::TopLeft | OverlayAnchor::TopCenter | OverlayAnchor::TopRight => {
                margin_top
            }
            OverlayAnchor::BottomLeft
            | OverlayAnchor::BottomCenter
            | OverlayAnchor::BottomRight => margin_top + avail_h.saturating_sub(height),
            _ => {
                // Center, LeftCenter, RightCenter, Bottom (shouldn't reach here for Bottom)
                margin_top + avail_h.saturating_sub(height) / 2
            }
        }
    }

    fn resolve_anchor_col(
        anchor: OverlayAnchor,
        width: usize,
        avail_w: usize,
        margin_left: usize,
    ) -> usize {
        match anchor {
            OverlayAnchor::TopLeft | OverlayAnchor::LeftCenter | OverlayAnchor::BottomLeft => {
                margin_left
            }
            OverlayAnchor::TopRight | OverlayAnchor::RightCenter | OverlayAnchor::BottomRight => {
                margin_left + avail_w.saturating_sub(width)
            }
            _ => {
                // Center, TopCenter, BottomCenter, Bottom
                margin_left + avail_w.saturating_sub(width) / 2
            }
        }
    }

    // -----------------------------------------------------------------------
    // Focus management (with Focusable support)
    // -----------------------------------------------------------------------

    /// Set focus to a specific child index, calling `set_focused()` on old/new.
    pub fn set_focus(&mut self, index: Option<usize>) {
        // Unfocus previously focused root child.
        if let Some(old_idx) = self.focus_index {
            if let Some(old) = self.root.children.get_mut(old_idx) {
                old.set_focused(false);
            }
        }
        self.focus_index = index;
        // Focus new root child.
        if let Some(new_idx) = index {
            if let Some(new_comp) = self.root.children.get_mut(new_idx) {
                new_comp.set_focused(true);
            }
        }
    }

    /// Get the currently focused child index.
    pub fn focus_index(&self) -> Option<usize> {
        self.focus_index
    }

    // -----------------------------------------------------------------------
    // Overlay stack management
    // -----------------------------------------------------------------------

    /// Show an overlay component with default (Bottom) anchor for legacy compat.
    /// Focus switches to the overlay. Returns the overlay's handle ID.
    pub fn show_overlay(&mut self, component: Box<dyn Component>) -> usize {
        self.show_overlay_with(
            component,
            OverlayOptions {
                anchor: OverlayAnchor::Bottom,
                ..Default::default()
            },
        )
    }

    /// Show an overlay component with explicit positioning options.
    /// Focus switches to the overlay unless `non_capturing` is set.
    /// Returns the overlay's handle ID (stack index at push time).
    pub fn show_overlay_with(
        &mut self,
        mut component: Box<dyn Component>,
        options: OverlayOptions,
    ) -> usize {
        let pre_focus = self.focus_index;
        let non_capturing = options.non_capturing;

        if !non_capturing {
            // Unfocus current root child or topmost overlay.
            self.unfocus_current();
            component.set_focused(true);
        }

        let id = self.overlay_stack.len();
        self.overlay_stack.push(OverlayEntry {
            component,
            pre_focus,
            hidden: false,
            non_capturing,
            options,
        });

        if !non_capturing {
            // Indicate overlay has focus (no root child focused).
            self.focus_index = None;
        }
        id
    }

    /// Hide (pop) the topmost overlay and restore focus to the previous target.
    pub fn hide_overlay(&mut self) {
        if let Some(mut entry) = self.overlay_stack.pop() {
            entry.component.set_focused(false);
            self.focus_index = entry.pre_focus;
            // Focus restored target.
            if let Some(idx) = self.focus_index {
                if let Some(child) = self.root.children.get_mut(idx) {
                    child.set_focused(true);
                }
            }
        }
    }

    /// Check if there are any visible (non-hidden) overlays.
    pub fn has_overlay(&self) -> bool {
        self.overlay_stack.iter().any(|e| !e.hidden)
    }

    /// Temporarily hide or show an overlay by its handle ID.
    pub fn set_overlay_hidden(&mut self, id: usize, hidden: bool) {
        // Validate and check early-out.
        let Some(entry) = self.overlay_stack.get(id) else {
            return;
        };
        if entry.hidden == hidden {
            return;
        }
        let non_capturing = entry.non_capturing;
        let pre_focus = entry.pre_focus;

        if hidden {
            // Unfocus the overlay component.
            self.overlay_stack[id].component.set_focused(false);
            self.overlay_stack[id].hidden = true;
            // Restore focus to pre_focus.
            self.focus_index = pre_focus;
            if let Some(idx) = self.focus_index {
                if let Some(child) = self.root.children.get_mut(idx) {
                    child.set_focused(true);
                }
            }
        } else {
            self.overlay_stack[id].hidden = false;
            if !non_capturing {
                // Unfocus whatever is currently focused first.
                self.unfocus_current();
                // Then focus the overlay.
                self.overlay_stack[id].component.set_focused(true);
                self.focus_index = None;
            }
        }
    }

    /// Unfocus whatever is currently focused (root child or topmost overlay).
    fn unfocus_current(&mut self) {
        // Check topmost capturing overlay first.
        if let Some(entry) = self
            .overlay_stack
            .iter_mut()
            .rev()
            .find(|e| !e.hidden && !e.non_capturing)
        {
            entry.component.set_focused(false);
            return;
        }
        // Then root children.
        if let Some(idx) = self.focus_index {
            if let Some(child) = self.root.children.get_mut(idx) {
                child.set_focused(false);
            }
        }
    }

    // -----------------------------------------------------------------------
    // Input listeners
    // -----------------------------------------------------------------------

    /// Add an input listener that can intercept/transform input before it
    /// reaches the focused component.
    pub fn add_input_listener(&mut self, listener: Box<dyn Fn(&str) -> InputResult + Send>) {
        self.input_listeners.push(listener);
    }

    // -----------------------------------------------------------------------
    // Input dispatch
    // -----------------------------------------------------------------------

    /// Dispatch a key event to the focused component (overlay or root child).
    /// Input listeners run first; if any returns `Consumed` dispatch is skipped.
    pub fn handle_key(&mut self, key: &KeyEvent) {
        // Run input listeners with a descriptive key string.
        let key_str = format!("{key:?}");
        for listener in &self.input_listeners {
            match listener(&key_str) {
                InputResult::Consumed => return,
                InputResult::Transformed(_) | InputResult::Ignored => {}
            }
        }

        // Shift+Ctrl+D → onDebug callback.
        if key.modifiers.contains(KeyModifiers::SHIFT | KeyModifiers::CONTROL)
            && key.code == KeyCode::Char('D')
        {
            if let Some(ref mut cb) = self.on_debug {
                cb();
                return;
            }
        }

        // If there's a visible capturing overlay on top, send input to it.
        if let Some(entry) = self.topmost_capturing_overlay_mut() {
            entry.component.handle_input(key);
            return;
        }
        // Otherwise dispatch to focused root child.
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

        // If there's a visible capturing overlay on top, send input to it.
        if let Some(entry) = self.topmost_capturing_overlay_mut() {
            entry.component.handle_raw_input(&current);
            return;
        }
        if let Some(idx) = self.focus_index {
            if let Some(child) = self.root.children.get_mut(idx) {
                child.handle_raw_input(&current);
            }
        }
    }

    /// Get a mutable reference to the topmost visible *capturing* overlay.
    /// Non-capturing overlays are skipped for input dispatch.
    fn topmost_capturing_overlay_mut(&mut self) -> Option<&mut OverlayEntry> {
        self.overlay_stack
            .iter_mut()
            .rev()
            .find(|e| !e.hidden && !e.non_capturing)
    }

    /// Downcast the topmost visible overlay to a concrete component type.
    pub fn topmost_overlay_as_mut<T: 'static>(&mut self) -> Option<&mut T> {
        self.overlay_stack
            .iter_mut()
            .rev()
            .find(|e| !e.hidden)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::component::Text;

    #[test]
    fn test_composite_line_at_basic() {
        let base = "Hello World Goodbye!";
        let overlay = "XXXXX";
        let result = TUI::composite_line_at(base, overlay, 6, 5, 20);
        // before="Hello " overlay="XXXXX" after="Goodbye!"
        assert!(result.contains("Hello "));
        assert!(result.contains("XXXXX"));
        assert!(result.contains("Goodbye!"));
    }

    #[test]
    fn test_composite_line_at_empty_base() {
        let result = TUI::composite_line_at("", "OK", 5, 4, 20);
        // Should pad before to 5, place "OK" padded to 4, then pad rest
        let vw = visible_width(&result);
        assert!(vw <= 20, "result width {vw} > 20");
    }

    #[test]
    fn test_resolve_layout_center() {
        let opts = OverlayOptions {
            anchor: OverlayAnchor::Center,
            ..Default::default()
        };
        let layout = TUI::resolve_overlay_layout(&opts, 5, 80, 24);
        // Centred: row ≈ (24 - 5) / 2 = 9, col ≈ (80 - 80) / 2 = 0
        assert!(layout.row > 0);
        assert!(layout.width > 0);
    }

    #[test]
    fn test_resolve_layout_top_left() {
        let opts = OverlayOptions {
            anchor: OverlayAnchor::TopLeft,
            width: Some(SizeValue::Absolute(40)),
            ..Default::default()
        };
        let layout = TUI::resolve_overlay_layout(&opts, 5, 80, 24);
        assert_eq!(layout.row, 0);
        assert_eq!(layout.col, 0);
        assert_eq!(layout.width, 40);
    }

    #[test]
    fn test_resolve_layout_percentage() {
        let opts = OverlayOptions {
            width: Some(SizeValue::Percent(50.0)),
            row: Some(SizeValue::Percent(25.0)),
            col: Some(SizeValue::Percent(50.0)),
            anchor: OverlayAnchor::Center,
            ..Default::default()
        };
        let layout = TUI::resolve_overlay_layout(&opts, 5, 80, 24);
        assert_eq!(layout.width, 40); // 50% of 80
    }

    #[test]
    fn test_set_focus_calls_set_focused() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        struct FocusTracker {
            focused: Arc<AtomicBool>,
        }
        impl Component for FocusTracker {
            fn render(&self, _w: u16) -> Vec<String> {
                vec![]
            }
            fn set_focused(&mut self, f: bool) {
                self.focused.store(f, Ordering::SeqCst);
            }
            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
            fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
                self
            }
        }

        let flag_a = Arc::new(AtomicBool::new(false));
        let flag_b = Arc::new(AtomicBool::new(false));
        let mut tui = TUI::new();
        tui.root.add(Box::new(FocusTracker {
            focused: flag_a.clone(),
        }));
        tui.root.add(Box::new(FocusTracker {
            focused: flag_b.clone(),
        }));

        tui.set_focus(Some(0));
        assert!(flag_a.load(Ordering::SeqCst));
        assert!(!flag_b.load(Ordering::SeqCst));

        tui.set_focus(Some(1));
        assert!(!flag_a.load(Ordering::SeqCst));
        assert!(flag_b.load(Ordering::SeqCst));

        tui.set_focus(None);
        assert!(!flag_b.load(Ordering::SeqCst));
    }

    #[test]
    fn test_non_capturing_overlay_keeps_focus() {
        let mut tui = TUI::new();
        tui.root.add(Box::new(Text::new("main")));
        tui.set_focus(Some(0));

        let _id = tui.show_overlay_with(
            Box::new(Text::new("popup")),
            OverlayOptions {
                non_capturing: true,
                ..Default::default()
            },
        );

        // Focus should stay on root child, not switch to overlay.
        assert_eq!(tui.focus_index(), Some(0));
    }

    #[test]
    fn test_capturing_overlay_steals_focus() {
        let mut tui = TUI::new();
        tui.root.add(Box::new(Text::new("main")));
        tui.set_focus(Some(0));

        tui.show_overlay(Box::new(Text::new("modal")));

        // Focus should be None (overlay has focus).
        assert_eq!(tui.focus_index(), None);

        tui.hide_overlay();
        // Restored.
        assert_eq!(tui.focus_index(), Some(0));
    }
}
