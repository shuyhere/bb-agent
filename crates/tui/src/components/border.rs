use std::sync::Arc;

use crate::component::Component;

pub type BorderColorFn = Arc<dyn Fn(&str) -> String + Send + Sync>;

/// Dynamic border component that adjusts to viewport width.
pub struct DynamicBorder {
    color: BorderColorFn,
}

impl DynamicBorder {
    pub fn new() -> Self {
        Self::with_color_fn(Arc::new(|text| text.to_string()))
    }

    pub fn with_color_fn(color: BorderColorFn) -> Self {
        Self { color }
    }

    pub fn set_color_fn(&mut self, color: BorderColorFn) {
        self.color = color;
    }
}

impl Default for DynamicBorder {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for DynamicBorder {
    fn render(&self, width: u16) -> Vec<String> {
        vec![(self.color)(&"─".repeat(width.max(1) as usize))]
    }

    fn invalidate(&mut self) {
        // No cached state to invalidate currently.
    }

    crate::impl_as_any!();
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    #[test]
    fn renders_at_least_one_cell() {
        assert_eq!(DynamicBorder::new().render(0), vec!["─"]);
    }

    #[test]
    fn applies_color_function() {
        let border = DynamicBorder::with_color_fn(Arc::new(|text| format!("<{text}>")));
        assert_eq!(border.render(3), vec!["<───>"]);
    }
}
