use std::sync::{Arc, Mutex};

use crate::component::Component;
use crate::utils::visible_width;

pub type BgFn = Arc<dyn Fn(&str) -> String + Send + Sync>;

#[derive(Clone, Debug)]
struct RenderCache {
    child_lines: Vec<String>,
    width: u16,
    bg_sample: Option<String>,
    lines: Vec<String>,
}

/// Box component - a container that applies padding and optional background to all children.
pub struct BoxComponent {
    children: Vec<Box<dyn Component>>,
    padding_x: u16,
    padding_y: u16,
    bg_fn: Option<BgFn>,
    cache: Mutex<Option<RenderCache>>,
}

impl BoxComponent {
    pub fn new(padding_x: u16, padding_y: u16) -> Self {
        Self {
            children: Vec::new(),
            padding_x,
            padding_y,
            bg_fn: None,
            cache: Mutex::new(None),
        }
    }

    pub fn with_bg_fn(padding_x: u16, padding_y: u16, bg_fn: BgFn) -> Self {
        Self {
            children: Vec::new(),
            padding_x,
            padding_y,
            bg_fn: Some(bg_fn),
            cache: Mutex::new(None),
        }
    }

    pub fn add_child(&mut self, component: Box<dyn Component>) {
        self.children.push(component);
        self.invalidate_cache();
    }

    pub fn remove_child(&mut self, index: usize) {
        if index < self.children.len() {
            self.children.remove(index);
            self.invalidate_cache();
        }
    }

    pub fn clear(&mut self) {
        self.children.clear();
        self.invalidate_cache();
    }

    pub fn set_bg_fn(&mut self, bg_fn: Option<BgFn>) {
        self.bg_fn = bg_fn;
        // Do not invalidate here - bg_fn changes are detected by sampling output.
    }

    pub fn children(&self) -> &[Box<dyn Component>] {
        &self.children
    }

    pub fn children_mut(&mut self) -> &mut [Box<dyn Component>] {
        &mut self.children
    }

    fn invalidate_cache(&self) {
        *self.cache.lock().expect("box cache poisoned") = None;
    }

    fn match_cache(&self, width: u16, child_lines: &[String], bg_sample: Option<&str>) -> Option<Vec<String>> {
        let cache = self.cache.lock().expect("box cache poisoned");
        let cache = cache.as_ref()?;

        if cache.width != width {
            return None;
        }
        if cache.bg_sample.as_deref() != bg_sample {
            return None;
        }
        if cache.child_lines.len() != child_lines.len() {
            return None;
        }
        if !cache
            .child_lines
            .iter()
            .zip(child_lines.iter())
            .all(|(cached, current)| cached == current)
        {
            return None;
        }

        Some(cache.lines.clone())
    }

    fn apply_bg(&self, line: &str, width: u16) -> String {
        let vis_len = visible_width(line);
        let pad_needed = (width as usize).saturating_sub(vis_len);
        let padded = format!("{line}{}", " ".repeat(pad_needed));

        match &self.bg_fn {
            Some(bg_fn) => bg_fn(&padded),
            None => padded,
        }
    }
}

impl Default for BoxComponent {
    fn default() -> Self {
        Self::new(1, 1)
    }
}

impl Component for BoxComponent {
    fn render(&self, width: u16) -> Vec<String> {
        if self.children.is_empty() {
            return Vec::new();
        }

        let horizontal_padding = self.padding_x.saturating_mul(2);
        let content_width = width.saturating_sub(horizontal_padding).max(1);
        let left_pad = " ".repeat(self.padding_x as usize);

        let mut child_lines = Vec::new();
        for child in &self.children {
            for line in child.render(content_width) {
                child_lines.push(format!("{left_pad}{line}"));
            }
        }

        if child_lines.is_empty() {
            return Vec::new();
        }

        let bg_sample = self.bg_fn.as_ref().map(|bg_fn| bg_fn("test"));
        if let Some(lines) = self.match_cache(width, &child_lines, bg_sample.as_deref()) {
            return lines;
        }

        let mut result = Vec::new();

        for _ in 0..self.padding_y {
            result.push(self.apply_bg("", width));
        }

        for line in &child_lines {
            result.push(self.apply_bg(line, width));
        }

        for _ in 0..self.padding_y {
            result.push(self.apply_bg("", width));
        }

        *self.cache.lock().expect("box cache poisoned") = Some(RenderCache {
            child_lines,
            width,
            bg_sample,
            lines: result.clone(),
        });

        result
    }

    fn invalidate(&mut self) {
        self.invalidate_cache();
        for child in &mut self.children {
            child.invalidate();
        }
    }

    crate::impl_as_any!();
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::component::Text;

    #[test]
    fn renders_children_with_padding() {
        let mut component = BoxComponent::new(1, 1);
        component.add_child(Box::new(Text::new("hello")));

        assert_eq!(
            component.render(8),
            vec!["        ", " hello  ", "        "]
        );
    }

    #[test]
    fn applies_background_to_full_line() {
        let mut component = BoxComponent::with_bg_fn(1, 0, Arc::new(|text| format!("<{text}>")));
        component.add_child(Box::new(Text::new("hi")));

        assert_eq!(component.render(6), vec!["< hi   >"]);
    }

    #[test]
    fn empty_children_render_empty() {
        let component = BoxComponent::new(1, 1);
        assert!(component.render(10).is_empty());
    }
}
