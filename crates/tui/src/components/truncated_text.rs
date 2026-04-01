use crate::component::Component;
use crate::impl_as_any;
use crate::utils::{truncate_to_width, visible_width};

/// Text component that truncates to fit viewport width.
pub struct TruncatedText {
    text: String,
    padding_x: usize,
    padding_y: usize,
}

impl TruncatedText {
    pub fn new(text: impl Into<String>, padding_x: usize, padding_y: usize) -> Self {
        Self {
            text: text.into(),
            padding_x,
            padding_y,
        }
    }
}

impl Component for TruncatedText {
    fn invalidate(&mut self) {
        // No cached state to invalidate currently
    }

    fn render(&self, width: u16) -> Vec<String> {
        let width = width as usize;
        let mut result = Vec::new();

        // Empty line padded to width
        let empty_line = " ".repeat(width);

        // Add vertical padding above
        for _ in 0..self.padding_y {
            result.push(empty_line.clone());
        }

        // Calculate available width after horizontal padding
        let available_width = std::cmp::max(1, width.saturating_sub(self.padding_x * 2));

        // Take only the first line (stop at newline)
        let mut single_line_text = self.text.as_str();
        if let Some(newline_index) = self.text.find('\n') {
            single_line_text = &self.text[..newline_index];
        }

        // Truncate text if needed (accounting for ANSI codes)
        let display_text = truncate_to_width(single_line_text, available_width);

        // Add horizontal padding
        let left_padding = " ".repeat(self.padding_x);
        let right_padding = " ".repeat(self.padding_x);
        let line_with_padding = format!("{}{}{}", left_padding, display_text, right_padding);

        // Pad line to exactly width characters
        let line_visible_width = visible_width(&line_with_padding);
        let padding_needed = width.saturating_sub(line_visible_width);
        let final_line = format!("{}{}", line_with_padding, " ".repeat(padding_needed));

        result.push(final_line);

        // Add vertical padding below
        for _ in 0..self.padding_y {
            result.push(empty_line.clone());
        }

        result
    }

    impl_as_any!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncates_and_pads_to_width() {
        let text = TruncatedText::new("hello world", 1, 0);
        let lines = text.render(8);
        assert_eq!(lines.len(), 1);
        assert_eq!(visible_width(&lines[0]), 8);
    }

    #[test]
    fn only_uses_the_first_line() {
        let text = TruncatedText::new("hello\nworld", 0, 0);
        assert_eq!(text.render(80), vec!["hello"]);
    }

    #[test]
    fn adds_vertical_padding() {
        let text = TruncatedText::new("hi", 0, 1);
        assert_eq!(text.render(4), vec!["    ", "hi  ", "    "]);
    }
}
