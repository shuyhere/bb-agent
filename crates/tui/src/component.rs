use crossterm::event::KeyEvent;

/// A renderable UI component.
pub trait Component: Send {
    /// Render this component into lines for the given terminal width.
    fn render(&self, width: u16) -> Vec<String>;

    /// Handle a key input event.
    fn handle_input(&mut self, _event: &KeyEvent) {}

    /// Mark this component as needing re-render.
    fn invalidate(&mut self) {}
}

/// A vertical container that renders children top-to-bottom.
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

    fn handle_input(&mut self, event: &KeyEvent) {
        for child in &mut self.children {
            child.handle_input(event);
        }
    }

    fn invalidate(&mut self) {
        for child in &mut self.children {
            child.invalidate();
        }
    }
}
