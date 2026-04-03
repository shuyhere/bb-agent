//! Component trait and Container — the building blocks of the TUI.
//!
//! Matches pi-tui's Component/Container model:
//! - Component renders to lines given a width
//! - Container holds children and renders them vertically
//! - Focusable components emit CURSOR_MARKER for hardware cursor positioning

use crossterm::event::KeyEvent;
use std::any::Any;

/// Zero-width cursor position marker (APC sequence).
/// Components emit this at the cursor position when focused.
/// TUI finds and strips this marker, then positions the hardware cursor there.
pub const CURSOR_MARKER: &str = "\x1b_bb:c\x07";

/// Marker line used to pin everything after it to the bottom of the viewport
/// when the rendered content is shorter than the terminal height.
pub const BOTTOM_ANCHOR_MARKER: &str = "\x1b_bb:b\x07";

/// A renderable TUI component.
pub trait Component: Send {
    /// Render to terminal lines for the given width.
    fn render(&self, width: u16) -> Vec<String>;

    /// Handle keyboard input when this component has focus.
    /// Receives a crossterm KeyEvent.
    fn handle_input(&mut self, _key: &KeyEvent) {}

    /// Handle raw input string (for bracketed paste, etc).
    fn handle_raw_input(&mut self, _data: &str) {}

    /// Invalidate cached rendering state (e.g., on theme/width change).
    fn invalidate(&mut self) {}

    /// Called when focus state changes. Components that implement Focusable
    /// should override this to track focus state and emit CURSOR_MARKER.
    /// Default is a no-op.
    fn set_focused(&mut self, _focused: bool) {}

    /// Downcast support.
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

/// Helper macro to implement as_any for a type.
#[macro_export]
macro_rules! impl_as_any {
    () => {
        fn as_any(&self) -> &dyn std::any::Any { self }
        fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
    };
}

/// Interface for components that can receive focus.
/// When focused, the component should emit CURSOR_MARKER in its render output.
pub trait Focusable {
    fn focused(&self) -> bool;
    fn set_focused(&mut self, focused: bool);
}

/// A container that renders children vertically.
pub struct Container {
    pub children: Vec<Box<dyn Component>>,
}

impl Container {
    pub fn new() -> Self {
        Self {
            children: Vec::new(),
        }
    }

    pub fn add(&mut self, child: Box<dyn Component>) {
        self.children.push(child);
    }

    pub fn remove(&mut self, index: usize) {
        if index < self.children.len() {
            self.children.remove(index);
        }
    }

    pub fn clear(&mut self) {
        self.children.clear();
    }

    pub fn len(&self) -> usize {
        self.children.len()
    }

    pub fn is_empty(&self) -> bool {
        self.children.is_empty()
    }
}

impl Default for Container {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for Container {
    fn render(&self, width: u16) -> Vec<String> {
        let mut lines = Vec::new();
        for child in &self.children {
            lines.extend(child.render(width));
        }
        lines
    }

    fn invalidate(&mut self) {
        for child in &mut self.children {
            child.invalidate();
        }
    }

    impl_as_any!();
}

/// A simple text component that renders static lines.
pub struct Text {
    pub lines: Vec<String>,
}

impl Text {
    pub fn new(text: &str) -> Self {
        Self {
            lines: text.lines().map(|l| l.to_string()).collect(),
        }
    }

    pub fn empty() -> Self {
        Self { lines: Vec::new() }
    }

    pub fn single(line: String) -> Self {
        Self { lines: vec![line] }
    }

    pub fn set(&mut self, text: &str) {
        self.lines = text.lines().map(|l| l.to_string()).collect();
    }
}

impl Component for Text {
    fn render(&self, _width: u16) -> Vec<String> {
        self.lines.clone()
    }

    fn invalidate(&mut self) {}
    impl_as_any!();
}

/// A spacer component that renders N empty lines.
pub struct Spacer {
    pub height: usize,
}

impl Spacer {
    pub fn new(height: usize) -> Self {
        Self { height }
    }
}

impl Component for Spacer {
    fn render(&self, _width: u16) -> Vec<String> {
        vec![String::new(); self.height]
    }

    fn invalidate(&mut self) {}
    impl_as_any!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_text_component() {
        let text = Text::new("hello\nworld");
        let lines = text.render(80);
        assert_eq!(lines, vec!["hello", "world"]);
    }

    #[test]
    fn test_container_renders_children() {
        let mut container = Container::new();
        container.add(Box::new(Text::new("line 1")));
        container.add(Box::new(Text::new("line 2")));
        let lines = container.render(80);
        assert_eq!(lines, vec!["line 1", "line 2"]);
    }

    #[test]
    fn test_spacer() {
        let spacer = Spacer::new(3);
        let lines = spacer.render(80);
        assert_eq!(lines.len(), 3);
        assert!(lines.iter().all(|l| l.is_empty()));
    }

    #[test]
    fn test_container_clear() {
        let mut container = Container::new();
        container.add(Box::new(Text::new("hello")));
        assert_eq!(container.len(), 1);
        container.clear();
        assert_eq!(container.len(), 0);
    }

    #[test]
    fn test_downcast_text() {
        let mut container = Container::new();
        container.add(Box::new(Text::new("hello")));
        let child = container.children[0].as_any_mut().downcast_mut::<Text>();
        assert!(child.is_some());
        child.unwrap().lines = vec!["world".to_string()];
        let lines = container.render(80);
        assert_eq!(lines, vec!["world"]);
    }
}
