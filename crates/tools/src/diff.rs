use similar::{ChangeTag, TextDiff};

pub struct DiffLine {
    pub tag: ChangeTag,
    pub content: String,
}

/// Generate a unified diff between old and new text, showing only context_lines
/// around each change (like `diff -U`). Unchanged regions are collapsed into
/// a single "..." separator.
pub fn generate_diff(old: &str, new: &str, context_lines: usize) -> Vec<DiffLine> {
    let diff = TextDiff::from_lines(old, new);
    let all_changes: Vec<(ChangeTag, String)> = diff
        .iter_all_changes()
        .map(|c| (c.tag(), c.value().to_string()))
        .collect();

    if all_changes.is_empty() {
        return Vec::new();
    }

    // Find which indices are "interesting" (added or removed) and mark their
    // context windows.
    let len = all_changes.len();
    let mut visible = vec![false; len];

    for (i, (tag, _)) in all_changes.iter().enumerate() {
        if *tag != ChangeTag::Equal {
            let start = i.saturating_sub(context_lines);
            let end = (i + context_lines + 1).min(len);
            for v in &mut visible[start..end] {
                *v = true;
            }
        }
    }

    let mut lines = Vec::new();
    let mut in_gap = false;

    for (i, (tag, content)) in all_changes.into_iter().enumerate() {
        if visible[i] {
            if in_gap {
                // Insert a separator for the skipped region
                lines.push(DiffLine {
                    tag: ChangeTag::Equal,
                    content: "...".to_string(),
                });
                in_gap = false;
            }
            lines.push(DiffLine { tag, content });
        } else {
            // This line is an equal line outside the context window
            in_gap = true;
        }
    }

    lines
}

/// Colors for rendering diff output.
pub struct DiffColors {
    pub added_fg: String,
    pub added_bg: String,
    pub removed_fg: String,
    pub removed_bg: String,
    pub context_fg: String,
    pub reset: String,
}

impl Default for DiffColors {
    /// Default colors matching the dark theme:
    /// - Added: green fg (#b5bd68) + subtle dark green bg (#1e3a1e)
    /// - Removed: red fg (#cc6666) + subtle dark red bg (#3a1e1e)
    /// - Context: gray fg (#808080), no bg
    fn default() -> Self {
        Self {
            added_fg: "\x1b[38;2;181;189;104m".into(),   // #b5bd68
            added_bg: "\x1b[48;2;30;58;30m".into(),      // #1e3a1e
            removed_fg: "\x1b[38;2;204;102;102m".into(), // #cc6666
            removed_bg: "\x1b[48;2;58;30;30m".into(),    // #3a1e1e
            context_fg: "\x1b[38;2;128;128;128m".into(), // #808080
            reset: "\x1b[0m".into(),
        }
    }
}

/// Render diff lines with ANSI colors and line numbers.
/// Only changed lines (+/-) get background highlight; context lines are plain.
pub fn render_diff(diff_lines: &[DiffLine]) -> Vec<String> {
    render_diff_colored(diff_lines, &DiffColors::default())
}

/// Render diff lines using the provided color scheme.
pub fn render_diff_colored(diff_lines: &[DiffLine], colors: &DiffColors) -> Vec<String> {
    if diff_lines.is_empty() {
        return Vec::new();
    }

    let max_line = estimate_max_line(diff_lines);
    let width = digit_count(max_line);

    let mut old_line: usize = 1;
    let mut new_line: usize = 1;
    let mut out = Vec::new();

    for line in diff_lines {
        if line.content == "..." && line.tag == ChangeTag::Equal {
            let pad = " ".repeat(width);
            out.push(format!("    {pad}  ..."));
            continue;
        }

        match line.tag {
            ChangeTag::Equal => {
                // Context lines: no background, just muted foreground
                let num = format!("{:>width$}", new_line, width = width);
                out.push(format!(
                    "    {} {num} {}{}",
                    colors.context_fg,
                    line.content.trim_end(),
                    colors.reset,
                ));
                old_line += 1;
                new_line += 1;
            }
            ChangeTag::Delete => {
                let num = format!("{:>width$}", old_line, width = width);
                out.push(format!(
                    "{}    {}-{num} {}{}",
                    colors.removed_bg,
                    colors.removed_fg,
                    line.content.trim_end(),
                    colors.reset,
                ));
                old_line += 1;
            }
            ChangeTag::Insert => {
                let num = format!("{:>width$}", new_line, width = width);
                out.push(format!(
                    "{}    {}+{num} {}{}",
                    colors.added_bg,
                    colors.added_fg,
                    line.content.trim_end(),
                    colors.reset,
                ));
                new_line += 1;
            }
        }
    }

    out
}

fn digit_count(n: usize) -> usize {
    if n == 0 {
        return 1;
    }
    ((n as f64).log10().floor() as usize) + 1
}

fn estimate_max_line(diff_lines: &[DiffLine]) -> usize {
    let mut old_line: usize = 1;
    let mut new_line: usize = 1;

    for line in diff_lines {
        if line.content == "..." && line.tag == ChangeTag::Equal {
            continue;
        }
        match line.tag {
            ChangeTag::Equal => {
                old_line += 1;
                new_line += 1;
            }
            ChangeTag::Delete => {
                old_line += 1;
            }
            ChangeTag::Insert => {
                new_line += 1;
            }
        }
    }
    old_line.max(new_line)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_diff_only_shows_context() {
        // 20-line file, change line 10 → should NOT show lines 1-5 or 16-20
        let old_lines: Vec<String> = (1..=20).map(|i| format!("line {i}")).collect();
        let mut new_lines = old_lines.clone();
        new_lines[9] = "modified line 10".to_string();
        let old = old_lines.join("\n") + "\n";
        let new = new_lines.join("\n") + "\n";

        let diff = generate_diff(&old, &new, 3);
        let rendered = render_diff(&diff);
        let text = rendered.join("\n");

        // Changed line must be present
        assert!(text.contains("modified line 10"));
        // Nearby context lines should be visible
        assert!(text.contains("line 8"));
        assert!(text.contains("line 13"));
        // Far-away lines should NOT be visible (only context ±3 around change)
        assert!(!text.contains("line 1 ") && !text.contains("line 1\n"));
        assert!(!text.contains("line 20"));
        // Should contain the "..." separator
        assert!(text.contains("..."));
    }

    #[test]
    fn test_small_diff_no_separator() {
        let old = "hello\n";
        let new = "world\n";
        let diff = generate_diff(old, new, 3);
        let rendered = render_diff(&diff);
        assert!(rendered.iter().any(|l| l.contains("hello")));
        assert!(rendered.iter().any(|l| l.contains("world")));
        // No "..." because the file is tiny
        assert!(!rendered.iter().any(|l| l.contains("...")));
    }

    #[test]
    fn test_render_diff_has_line_numbers() {
        let old = "aaa\nbbb\nccc\n";
        let new = "aaa\nBBB\nccc\n";
        let diff = generate_diff(old, new, 1);
        let rendered = render_diff(&diff);
        let text = rendered.join("\n");
        // Should show line numbers
        assert!(text.contains("1"));
        assert!(text.contains("2"));
    }

    #[test]
    fn test_multiple_changes_separate_contexts() {
        let old_lines: Vec<String> = (1..=50).map(|i| format!("line {i}")).collect();
        let mut new_lines = old_lines.clone();
        new_lines[4] = "changed line 5".to_string();
        new_lines[44] = "changed line 45".to_string();
        let old = old_lines.join("\n") + "\n";
        let new = new_lines.join("\n") + "\n";

        let diff = generate_diff(&old, &new, 2);
        let rendered = render_diff(&diff);
        let text = rendered.join("\n");

        // Both changes visible
        assert!(text.contains("changed line 5"));
        assert!(text.contains("changed line 45"));
        // Middle lines (e.g. line 20) should be hidden
        assert!(!text.contains("line 20"));
        // Should have "..." separator(s)
        assert!(text.contains("..."));
    }
}
