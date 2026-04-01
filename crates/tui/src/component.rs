//! Component trait and Container — the building blocks of the TUI.
//!
//! Matches pi-tui's Component/Container model:
//! - Component renders to lines given a width
//! - Container holds children and renders them vertically

/// A renderable TUI component.
pub trait Component {
    /// Render to terminal lines for the given width.
    fn render(&self, width: u16) -> Vec<String>;

    /// Handle keyboard input (when focused). Called with raw crossterm data.
    fn handle_input(&mut self, _key: &crossterm::event::KeyEvent) {}

    /// Invalidate cached rendering state (e.g., on theme/width change).
    fn invalidate(&mut self) {}
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

    pub fn clear(&mut self) {
        self.children.clear();
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
}
