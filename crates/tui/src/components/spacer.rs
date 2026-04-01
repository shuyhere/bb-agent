use crate::component::Component;
use crate::impl_as_any;

/// Spacer component that renders empty lines.
pub struct Spacer {
    lines: usize,
}

impl Spacer {
    pub fn new(lines: usize) -> Self {
        Self { lines }
    }

    pub fn set_lines(&mut self, lines: usize) {
        self.lines = lines;
    }
}

impl Component for Spacer {
    fn invalidate(&mut self) {
        // No cached state to invalidate currently
    }

    fn render(&self, _width: u16) -> Vec<String> {
        let mut result = Vec::with_capacity(self.lines);
        for _ in 0..self.lines {
            result.push(String::new());
        }
        result
    }

    impl_as_any!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_requested_number_of_empty_lines() {
        let spacer = Spacer::new(3);
        assert_eq!(spacer.render(80), vec!["", "", ""]);
    }
}
