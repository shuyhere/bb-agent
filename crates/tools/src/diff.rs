use similar::{ChangeTag, TextDiff};

pub struct DiffLine {
    pub tag: ChangeTag,
    pub content: String,
}

/// Generate a diff between old and new text.
pub fn generate_diff(old: &str, new: &str, _context_lines: usize) -> Vec<DiffLine> {
    let diff = TextDiff::from_lines(old, new);
    let mut lines = Vec::new();

    for change in diff.iter_all_changes() {
        lines.push(DiffLine {
            tag: change.tag(),
            content: change.value().to_string(),
        });
    }

    lines
}

/// Render diff lines with ANSI colors.
pub fn render_diff(diff_lines: &[DiffLine]) -> Vec<String> {
    diff_lines
        .iter()
        .map(|line| {
            let prefix = match line.tag {
                ChangeTag::Equal => "  ",
                ChangeTag::Delete => "- ",
                ChangeTag::Insert => "+ ",
            };
            let color = match line.tag {
                ChangeTag::Equal => "",
                ChangeTag::Delete => "\x1b[31m",
                ChangeTag::Insert => "\x1b[32m",
            };
            let reset = if color.is_empty() { "" } else { "\x1b[0m" };
            format!(
                "    {}{}{}{}",
                color,
                prefix,
                line.content.trim_end(),
                reset
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_diff() {
        let old = "line1\nline2\nline3\n";
        let new = "line1\nmodified\nline3\n";
        let diff = generate_diff(old, new, 1);
        assert!(diff.iter().any(|l| l.tag == ChangeTag::Delete));
        assert!(diff.iter().any(|l| l.tag == ChangeTag::Insert));
    }

    #[test]
    fn test_render_diff() {
        let old = "hello\n";
        let new = "world\n";
        let diff = generate_diff(old, new, 0);
        let rendered = render_diff(&diff);
        assert!(rendered.iter().any(|l| l.contains("hello")));
        assert!(rendered.iter().any(|l| l.contains("world")));
    }
}
