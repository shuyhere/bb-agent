use super::*;

#[test]
fn test_basic_render() {
    let md = "# Hello\n\nThis is a paragraph.\n\n## Subheading\n\n- item 1\n- item 2\n";
    let mut renderer = MarkdownRenderer::new(md);
    let lines = renderer.render(80);
    assert!(
        lines.len() >= 5,
        "Expected at least 5 lines, got {}",
        lines.len()
    );
    // Check heading is present
    let joined = lines.join("\n");
    assert!(joined.contains("Hello"));
    assert!(joined.contains("Subheading"));
    assert!(joined.contains("item 1"));
}

#[test]
fn test_heading_styles_use_distinct_levels() {
    let md = "# Hello\n\n## Subheading\n";
    let mut renderer = MarkdownRenderer::new(md);
    let lines = renderer.render(80);
    let joined = lines.join("\n");
    assert!(joined.contains(*HEADING_H1));
    assert!(joined.contains(*HEADING_H2));
}

#[test]
fn test_code_block() {
    let md = "```rust\nfn main() {}\n```\n";
    let mut renderer = MarkdownRenderer::new(md);
    let lines = renderer.render(80);
    // Should have top border, code line, bottom border
    assert!(
        lines.len() >= 3,
        "Expected at least 3 lines for code block, got {}",
        lines.len()
    );
    let joined = lines.join("\n");
    assert!(joined.contains("main"));
}

#[test]
fn test_word_wrap() {
    let long = "word ".repeat(20);
    let md = format!("{}\n", long.trim());
    let mut renderer = MarkdownRenderer::new(&md);
    let lines = renderer.render(30);
    assert!(lines.len() > 1, "Expected wrapping at width 30");
}

#[test]
fn test_inline_formatting() {
    let md = "This is **bold** and *italic* and ~~struck~~.\n";
    let mut renderer = MarkdownRenderer::new(md);
    let lines = renderer.render(80);
    let joined = lines.join("\n");
    assert!(joined.contains("bold"));
    assert!(joined.contains("italic"));
    assert!(joined.contains("struck"));
}

#[test]
fn test_blockquote() {
    let md = "> This is a quote\n";
    let mut renderer = MarkdownRenderer::new(md);
    let lines = renderer.render(80);
    let joined = lines.join("\n");
    assert!(joined.contains("│"));
    assert!(joined.contains("quote"));
    assert!(joined.contains(ITALIC));
}

#[test]
fn test_ordered_list() {
    let md = "1. first\n2. second\n3. third\n";
    let mut renderer = MarkdownRenderer::new(md);
    let lines = renderer.render(80);
    let joined = lines.join("\n");
    assert!(joined.contains("1."));
    assert!(joined.contains("first"));
}

#[test]
fn test_nested_ordered_list_markers_change_by_depth() {
    let md = "1. top\n   1. nested\n      1. alpha\n         1. roman\n";
    let mut renderer = MarkdownRenderer::new(md);
    let lines = renderer.render(80);
    let plain = lines
        .iter()
        .map(|line| strip_ansi(line))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(plain.contains("1. top"));
    assert!(plain.contains("1. nested"));
    assert!(plain.contains("a. alpha"));
    assert!(plain.contains("i. roman"));
}

#[test]
fn test_plain_url_is_not_treated_as_markdown() {
    let text = "https://example.com/some-long-path-with-dashes?foo=bar";
    assert!(!has_markdown_syntax(text));

    let mut renderer = MarkdownRenderer::new(text);
    let lines = renderer.render(80);
    let joined = lines.join("\n");
    assert!(joined.contains(text));
    assert!(!joined.contains("─"));
}

#[test]
fn test_plain_text_with_hyphenated_url_does_not_render_horizontal_rule() {
    let text = "See https://example.com/my-long-url-with-dashes for details.";
    let mut renderer = MarkdownRenderer::new(text);
    let lines = renderer.render(80);
    let joined = lines.join("\n");
    assert!(joined.contains("https://example.com/my-long-url-with-dashes"));
    assert!(!joined.contains("─"));
}

#[test]
fn test_horizontal_rule_still_renders_when_explicit() {
    let md = "---\n";
    let mut renderer = MarkdownRenderer::new(md);
    let lines = renderer.render(40);
    let joined = lines.join("\n");
    assert!(joined.contains("─"));
}

#[test]
fn test_setext_heading_detection_still_counts_as_markdown() {
    let md = "Heading\n---\n";
    assert!(has_markdown_syntax(md));
}

#[test]
fn test_caching() {
    let md = "# Test\n";
    let mut renderer = MarkdownRenderer::new(md);
    let lines1 = renderer.render(80);
    let lines2 = renderer.render(80);
    assert_eq!(lines1, lines2);
    // Different width should re-render
    let _lines3 = renderer.render(40);
    assert!(renderer.cached_lines.as_ref().unwrap().0 == 40);
}

#[test]
fn test_strip_ansi() {
    assert_eq!(strip_ansi("\x1b[1mhello\x1b[0m"), "hello");
    assert_eq!(strip_ansi("no codes"), "no codes");
}

#[test]
fn test_visible_width() {
    assert_eq!(visible_width("\x1b[1mhi\x1b[0m"), 2);
}

#[test]
fn test_link() {
    let md = "[click here](https://example.com)\n";
    let mut renderer = MarkdownRenderer::new(md);
    let lines = renderer.render(80);
    let joined = lines.join("\n");
    assert!(joined.contains("click here"));
    assert!(joined.contains("\x1b]8;;https://example.com\x07"));
    assert!(joined.contains("\x1b]8;;\x07"));
}

#[test]
fn test_github_reference_autolinks() {
    let md = "See owner/repo#123 for details.\n";
    let mut renderer = MarkdownRenderer::new(md);
    let lines = renderer.render(80);
    let joined = lines.join("\n");
    assert!(joined.contains("owner/repo#123"));
    assert!(joined.contains("\x1b]8;;https://github.com/owner/repo/issues/123\x07"));
}

#[test]
fn test_issue_only_reference_autolinks_with_current_repo() {
    let rendered = auto_link_github_refs_with_repo("See #123 for details.", Some("owner/repo"));
    assert!(rendered.contains("#123"));
    assert!(rendered.contains("\x1b]8;;https://github.com/owner/repo/issues/123\x07"));
}

#[test]
fn test_inline_code() {
    let md = "Use `cargo build` to compile.\n";
    let mut renderer = MarkdownRenderer::new(md);
    let lines = renderer.render(80);
    let joined = lines.join("\n");
    assert!(joined.contains("cargo build"));
    assert!(joined.contains(CODE_INLINE));
}

#[test]
fn test_approx_tilde_is_not_strikethrough() {
    let md = "About ~100 files.\n";
    let mut renderer = MarkdownRenderer::new(md);
    let lines = renderer.render(80);
    let joined = lines.join("\n");
    assert!(joined.contains("~100"));
    assert!(!joined.contains(STRIKETHROUGH));
}

#[test]
fn test_markdown_block_tokens_split_stable_blocks() {
    let text = "# One\n\nTwo\n\n```rs\nfn main() {}\n```\n";
    let tokens = markdown_block_tokens(text);
    assert_eq!(tokens.len(), 3);
    assert!(tokens[0].text.contains("# One"));
    assert!(tokens[1].text.contains("Two"));
    assert!(tokens[2].text.contains("fn main()"));
}

#[test]
fn test_table_rows_do_not_collapse_together() {
    let md =
        "| Crate | Purpose |\n| --- | --- |\n| cli | CLI entry point |\n| core | Agent loop |\n";
    let mut renderer = MarkdownRenderer::new(md);
    let lines = renderer.render(80);
    let plain = lines
        .iter()
        .map(|line| strip_ansi(line))
        .collect::<Vec<_>>();
    let joined = plain.join("\n");
    assert!(
        plain
            .iter()
            .any(|line| line.contains("Crate") && line.contains("Purpose"))
    );
    assert!(
        plain
            .iter()
            .any(|line| line.contains("cli") && line.contains("CLI entry point"))
    );
    assert!(
        plain
            .iter()
            .any(|line| line.contains("core") && line.contains("Agent loop"))
    );
    assert!(joined.contains("┼") || joined.contains("│"));
    assert!(!joined.contains("CratePurposecliCLI"));
}

#[test]
fn test_table_keeps_inline_markdown_styling() {
    let md = "| Crate | Purpose |\n| --- | --- |\n| `cli` | **CLI** entry point |\n";
    let mut renderer = MarkdownRenderer::new(md);
    let lines = renderer.render(80);
    let joined = lines.join("\n");
    assert!(joined.contains(CODE_INLINE));
    assert!(joined.contains(BOLD));
}
