use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle, ThemeSet};
use syntect::parsing::SyntaxSet;
use unicode_width::UnicodeWidthStr;

// ANSI escape codes
const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const ITALIC: &str = "\x1b[3m";
const STRIKETHROUGH: &str = "\x1b[9m";
const HEADING_H1: &str = "\x1b[1;97m";
const HEADING_H2: &str = "\x1b[1;36m";
const HEADING_H3: &str = "\x1b[1;33m";
const HEADING_DEFAULT: &str = "\x1b[1;37m";
const CODE_INLINE: &str = "\x1b[38;5;223m";
const CODE_BORDER: &str = "\x1b[90m";
const QUOTE_PREFIX: &str = "\x1b[90m│\x1b[0m ";
const BULLET: &str = "\x1b[90m•\x1b[0m";
const LINK_URL: &str = "\x1b[4;90m";
const DIM: &str = "\x1b[2m";

pub struct MarkdownRenderer {
    text: String,
    cached_lines: Option<(u16, Vec<String>)>,
}

impl MarkdownRenderer {
    pub fn new(text: &str) -> Self {
        Self {
            text: text.to_string(),
            cached_lines: None,
        }
    }

    pub fn set_text(&mut self, text: &str) {
        if self.text != text {
            self.text = text.to_string();
            self.cached_lines = None;
        }
    }

    pub fn render(&mut self, width: u16) -> Vec<String> {
        if let Some((cached_width, ref lines)) = self.cached_lines {
            if cached_width == width {
                return lines.clone();
            }
        }
        let lines = render_markdown(&self.text, width);
        self.cached_lines = Some((width, lines.clone()));
        lines
    }
}

/// Internal state for the markdown-to-lines converter.
struct RenderState {
    lines: Vec<String>,
    width: u16,
    // Current inline buffer being built
    current_line: String,
    // Active style stack (escape codes to re-apply after wrap)
    style_stack: Vec<&'static str>,
    // Heading level (None if not in heading)
    heading: Option<HeadingLevel>,
    // In code block
    code_block: Option<String>, // language
    code_block_lines: Vec<String>,
    // Block quote depth
    quote_depth: usize,
    // List stack: None = unordered, Some(n) = ordered starting at n
    list_stack: Vec<Option<u64>>,
    // Current item index in ordered list
    list_counters: Vec<u64>,
    // Link state
    link_url: Option<String>,
    // Whether we are inside a paragraph
    in_paragraph: bool,
    // Syntect lazy-loaded
    syntax_set: SyntaxSet,
    theme_set: ThemeSet,
}

impl RenderState {
    fn new(width: u16) -> Self {
        Self {
            lines: Vec::new(),
            width,
            current_line: String::new(),
            style_stack: Vec::new(),
            heading: None,
            code_block: None,
            code_block_lines: Vec::new(),
            quote_depth: 0,
            list_stack: Vec::new(),
            list_counters: Vec::new(),
            link_url: None,
            in_paragraph: false,
            syntax_set: SyntaxSet::load_defaults_newlines(),
            theme_set: ThemeSet::load_defaults(),
        }
    }

    fn push_style(&mut self, code: &'static str) {
        self.style_stack.push(code);
        self.current_line.push_str(code);
    }

    fn pop_style(&mut self) {
        self.style_stack.pop();
        self.current_line.push_str(RESET);
        // Re-apply remaining styles
        for &s in &self.style_stack {
            self.current_line.push_str(s);
        }
    }

    fn active_styles(&self) -> String {
        self.style_stack.iter().copied().collect()
    }

    /// Compute line prefix for quotes and list nesting
    fn line_prefix(&self) -> String {
        let mut prefix = String::new();
        // Quote prefix
        for _ in 0..self.quote_depth {
            prefix.push_str(QUOTE_PREFIX);
        }
        prefix
    }

    /// Compute prefix width (visible characters)
    fn prefix_width(&self) -> usize {
        // Each quote level adds "│ " = 2 visible chars
        self.quote_depth * 2
    }

    /// Get list indent prefix for the current nesting level
    fn list_indent(&self) -> String {
        // Each list level adds 2 spaces of indent
        "  ".repeat(self.list_stack.len().saturating_sub(1))
    }

    #[allow(dead_code)]
    fn list_indent_width(&self) -> usize {
        self.list_stack.len().saturating_sub(1) * 2
    }

    /// Flush current_line buffer to lines, performing word-wrap
    fn flush_line(&mut self) {
        if self.current_line.is_empty() {
            return;
        }
        let text = std::mem::take(&mut self.current_line);
        let prefix = self.line_prefix();
        let prefix_w = self.prefix_width();
        let avail = (self.width as usize).saturating_sub(prefix_w);
        let wrapped = word_wrap_ansi(&text, avail);
        for line in wrapped {
            self.lines.push(format!("{}{}", prefix, line));
        }
    }

    /// Push a blank separator line
    fn push_blank(&mut self) {
        // Don't push blank at the very start or after another blank
        if !self.lines.is_empty() {
            let last = self.lines.last().unwrap();
            if !strip_ansi(last).trim().is_empty() {
                let prefix = self.line_prefix();
                self.lines.push(prefix);
            }
        }
    }

    fn push_text(&mut self, text: &str) {
        self.current_line.push_str(text);
    }

    /// Render code block with syntax highlighting
    fn render_code_block(&mut self) {
        let lang = self.code_block.take().unwrap_or_default();
        let code_lines = std::mem::take(&mut self.code_block_lines);
        let prefix = self.line_prefix();
        let prefix_w = self.prefix_width();
        let avail = (self.width as usize).saturating_sub(prefix_w);

        // Top border with language label
        let label = if lang.is_empty() {
            String::new()
        } else {
            format!(" {} ", lang)
        };
        let border_len = avail.saturating_sub(label.len()).saturating_sub(2);
        let top_border = format!(
            "{}{}┌{}{}┐{}",
            prefix,
            CODE_BORDER,
            label,
            "─".repeat(border_len),
            RESET
        );
        self.lines.push(top_border);

        // Syntax highlight
        let syntax = self
            .syntax_set
            .find_syntax_by_token(&lang)
            .unwrap_or_else(|| self.syntax_set.find_syntax_plain_text());
        let theme = &self.theme_set.themes["base16-ocean.dark"];
        let mut highlighter = HighlightLines::new(syntax, theme);

        for code_line in &code_lines {
            let highlighted = highlighter
                .highlight_line(code_line, &self.syntax_set)
                .unwrap_or_default();
            let mut styled = String::new();
            for (style, text) in highlighted {
                let fg = style.foreground;
                styled.push_str(&format!("\x1b[38;2;{};{};{}m", fg.r, fg.g, fg.b));
                if style.font_style.contains(FontStyle::BOLD) {
                    styled.push_str(BOLD);
                }
                if style.font_style.contains(FontStyle::ITALIC) {
                    styled.push_str(ITALIC);
                }
                styled.push_str(text);
                styled.push_str(RESET);
            }
            let inner_width = avail.saturating_sub(4); // "│ " on each side
            // Truncate if needed (simplified - no wrap inside code blocks)
            let vis_width = visible_width(&styled);
            let padding = if inner_width > vis_width {
                " ".repeat(inner_width - vis_width)
            } else {
                String::new()
            };
            self.lines.push(format!(
                "{}{}│ {}{}{}│{}",
                prefix, CODE_BORDER, styled, padding, CODE_BORDER, RESET
            ));
        }

        // Bottom border
        let bottom_border = format!(
            "{}{}└{}┘{}",
            prefix,
            CODE_BORDER,
            "─".repeat(avail.saturating_sub(2)),
            RESET
        );
        self.lines.push(bottom_border);
    }
}

fn render_markdown(text: &str, width: u16) -> Vec<String> {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_TASKLISTS);

    let parser = Parser::new_ext(text, opts);
    let mut state = RenderState::new(width);

    for event in parser {
        match event {
            Event::Start(tag) => handle_start(&mut state, tag),
            Event::End(tag) => handle_end(&mut state, tag),
            Event::Text(text) => handle_text(&mut state, &text),
            Event::Code(code) => handle_inline_code(&mut state, &code),
            Event::SoftBreak => {
                if state.code_block.is_some() {
                    state.code_block_lines.push(String::new());
                } else {
                    state.push_text(" ");
                }
            }
            Event::HardBreak => {
                state.flush_line();
            }
            Event::Rule => {
                state.flush_line();
                state.push_blank();
                let prefix = state.line_prefix();
                let prefix_w = state.prefix_width();
                let avail = (state.width as usize).saturating_sub(prefix_w);
                state
                    .lines
                    .push(format!("{}{}{}{}", prefix, DIM, "─".repeat(avail), RESET));
                state.push_blank();
            }
            Event::TaskListMarker(checked) => {
                let marker = if checked { "☑ " } else { "☐ " };
                state.push_text(marker);
            }
            _ => {}
        }
    }

    // Flush remaining
    state.flush_line();
    state.lines
}

fn handle_start(state: &mut RenderState, tag: Tag) {
    match tag {
        Tag::Paragraph => {
            if state.in_paragraph {
                state.flush_line();
            }
            state.in_paragraph = true;
            if !state.lines.is_empty() && state.list_stack.is_empty() {
                state.push_blank();
            }
        }
        Tag::Heading { level, .. } => {
            state.flush_line();
            state.push_blank();
            state.heading = Some(level);
            let style = match level {
                HeadingLevel::H1 => HEADING_H1,
                HeadingLevel::H2 => HEADING_H2,
                HeadingLevel::H3 => HEADING_H3,
                _ => HEADING_DEFAULT,
            };
            state.push_style(style);
            // Add heading marker
            let marker = match level {
                HeadingLevel::H1 => "# ",
                HeadingLevel::H2 => "## ",
                HeadingLevel::H3 => "### ",
                HeadingLevel::H4 => "#### ",
                HeadingLevel::H5 => "##### ",
                HeadingLevel::H6 => "###### ",
            };
            state.push_text(marker);
        }
        Tag::BlockQuote(_) => {
            state.flush_line();
            state.quote_depth += 1;
        }
        Tag::CodeBlock(kind) => {
            state.flush_line();
            state.push_blank();
            let lang = match kind {
                CodeBlockKind::Fenced(lang) => lang.to_string(),
                CodeBlockKind::Indented => String::new(),
            };
            state.code_block = Some(lang);
            state.code_block_lines = Vec::new();
        }
        Tag::List(start) => {
            state.flush_line();
            if state.list_stack.is_empty() {
                state.push_blank();
            }
            state.list_stack.push(start);
            state.list_counters.push(start.unwrap_or(0));
        }
        Tag::Item => {
            state.flush_line();
            let indent = state.list_indent();

            let is_ordered = state
                .list_stack
                .last()
                .map(|s| s.is_some())
                .unwrap_or(false);

            if is_ordered {
                let counter_val = {
                    let c = state.list_counters.last_mut().unwrap();
                    let v = *c;
                    *c += 1;
                    v
                };
                state.push_text(&format!("{}{}. ", indent, counter_val));
            } else {
                state.push_text(&format!("{}{} ", indent, BULLET));
            }
        }
        Tag::Strong => {
            state.push_style(BOLD);
        }
        Tag::Emphasis => {
            state.push_style(ITALIC);
        }
        Tag::Strikethrough => {
            state.push_style(STRIKETHROUGH);
        }
        Tag::Link { dest_url, .. } => {
            state.link_url = Some(dest_url.to_string());
        }
        _ => {}
    }
}

fn handle_end(state: &mut RenderState, tag: TagEnd) {
    match tag {
        TagEnd::Paragraph => {
            state.flush_line();
            state.in_paragraph = false;
        }
        TagEnd::Heading(_) => {
            state.pop_style();
            state.flush_line();
            state.heading = None;
        }
        TagEnd::BlockQuote(_) => {
            state.flush_line();
            state.quote_depth = state.quote_depth.saturating_sub(1);
        }
        TagEnd::CodeBlock => {
            state.render_code_block();
            state.push_blank();
        }
        TagEnd::List(_) => {
            state.flush_line();
            state.list_stack.pop();
            state.list_counters.pop();
            if state.list_stack.is_empty() {
                state.push_blank();
            }
        }
        TagEnd::Item => {
            state.flush_line();
        }
        TagEnd::Strong => {
            state.pop_style();
        }
        TagEnd::Emphasis => {
            state.pop_style();
        }
        TagEnd::Strikethrough => {
            state.pop_style();
        }
        TagEnd::Link => {
            if let Some(url) = state.link_url.take() {
                state.push_text(&format!(" ({}{}{})", LINK_URL, url, RESET));
                // Re-apply active styles after reset
                let styles = state.active_styles();
                if !styles.is_empty() {
                    state.push_text(&styles);
                }
            }
        }
        _ => {}
    }
}

fn handle_text(state: &mut RenderState, text: &str) {
    if state.code_block.is_some() {
        // Collect code block lines
        for line in text.split('\n') {
            state.code_block_lines.push(line.to_string());
        }
        // If text ends with \n, last push was an empty string representing the trailing newline.
        // Remove it to avoid extra blank line.
        if text.ends_with('\n') && !state.code_block_lines.is_empty() {
            state.code_block_lines.pop();
        }
    } else {
        state.push_text(text);
    }
}

fn handle_inline_code(state: &mut RenderState, code: &str) {
    state.push_text(&format!("{}`{}`{}", CODE_INLINE, code, RESET));
    // Re-apply active styles
    let styles = state.active_styles();
    if !styles.is_empty() {
        state.push_text(&styles);
    }
}

/// Strip ANSI escape codes to get visible text
fn strip_ansi(s: &str) -> String {
    let mut result = String::new();
    let mut in_escape = false;
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            in_escape = true;
            continue;
        }
        if in_escape {
            if c.is_ascii_alphabetic() {
                in_escape = false;
            }
            continue;
        }
        result.push(c);
    }
    result
}

/// Compute visible width of a string with ANSI codes
fn visible_width(s: &str) -> usize {
    UnicodeWidthStr::width(strip_ansi(s).as_str())
}

/// Word-wrap text with ANSI codes to fit within `max_width`.
fn word_wrap_ansi(text: &str, max_width: usize) -> Vec<String> {
    if max_width == 0 {
        return vec![text.to_string()];
    }

    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut current_width: usize = 0;

    // Track active ANSI codes so we can carry them across lines
    let mut active_codes: Vec<String> = Vec::new();

    // Split into segments: ANSI codes and visible text
    let segments = split_ansi_segments(text);

    for segment in segments {
        if segment.starts_with('\x1b') {
            // It's an ANSI escape code
            current.push_str(&segment);
            // Track it
            if segment.contains("[0m") {
                active_codes.clear();
            } else {
                active_codes.push(segment);
            }
            continue;
        }

        // Visible text - word wrap it
        for word in WordSplitter::new(&segment) {
            let word_w = UnicodeWidthStr::width(word);

            if current_width + word_w > max_width && current_width > 0 {
                // Wrap: close current line and start new one
                current.push_str(RESET);
                lines.push(current);
                current = active_codes.join("");

                // Skip leading space on new line
                let trimmed = word.trim_start();
                let trimmed_w = UnicodeWidthStr::width(trimmed);
                current.push_str(trimmed);
                current_width = trimmed_w;
            } else {
                current.push_str(word);
                current_width += word_w;
            }
        }
    }

    if !current.is_empty() {
        lines.push(current);
    }

    if lines.is_empty() {
        lines.push(String::new());
    }

    lines
}

/// Split a string into ANSI escape segments and text segments.
fn split_ansi_segments(text: &str) -> Vec<String> {
    let mut segments = Vec::new();
    let mut current = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Flush current text segment
            if !current.is_empty() {
                segments.push(std::mem::take(&mut current));
            }
            // Start collecting escape sequence
            let mut esc = String::new();
            esc.push(c);
            while let Some(&nc) = chars.peek() {
                esc.push(nc);
                chars.next();
                if nc.is_ascii_alphabetic() {
                    break;
                }
            }
            segments.push(esc);
        } else {
            current.push(c);
        }
    }

    if !current.is_empty() {
        segments.push(current);
    }

    segments
}

/// Helper to split text into words while preserving spaces as part of the word.
struct WordSplitter<'a> {
    remaining: &'a str,
}

impl<'a> WordSplitter<'a> {
    fn new(s: &'a str) -> Self {
        Self { remaining: s }
    }
}

impl<'a> Iterator for WordSplitter<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining.is_empty() {
            return None;
        }

        // Find next space boundary
        let bytes = self.remaining.as_bytes();
        // Find end of current chunk (non-space then space, or space then non-space)
        let mut i = 0;
        let starts_with_space = bytes[0] == b' ';

        if starts_with_space {
            // Consume all spaces
            while i < bytes.len() && bytes[i] == b' ' {
                i += 1;
            }
        } else {
            // Consume until space
            while i < bytes.len() && bytes[i] != b' ' {
                i += 1;
            }
        }

        let (chunk, rest) = self.remaining.split_at(i);
        self.remaining = rest;
        Some(chunk)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_render() {
        let md = "# Hello\n\nThis is a paragraph.\n\n## Subheading\n\n- item 1\n- item 2\n";
        let mut renderer = MarkdownRenderer::new(md);
        let lines = renderer.render(80);
        assert!(lines.len() >= 5, "Expected at least 5 lines, got {}", lines.len());
        // Check heading is present
        let joined = lines.join("\n");
        assert!(joined.contains("Hello"));
        assert!(joined.contains("Subheading"));
        assert!(joined.contains("item 1"));
    }

    #[test]
    fn test_code_block() {
        let md = "```rust\nfn main() {}\n```\n";
        let mut renderer = MarkdownRenderer::new(md);
        let lines = renderer.render(80);
        // Should have top border, code line, bottom border
        assert!(lines.len() >= 3, "Expected at least 3 lines for code block, got {}", lines.len());
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
    fn test_horizontal_rule() {
        let md = "---\n";
        let mut renderer = MarkdownRenderer::new(md);
        let lines = renderer.render(40);
        let joined = lines.join("\n");
        assert!(joined.contains("─"));
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
        assert!(joined.contains("example.com"));
    }

    #[test]
    fn test_inline_code() {
        let md = "Use `cargo build` to compile.\n";
        let mut renderer = MarkdownRenderer::new(md);
        let lines = renderer.render(80);
        let joined = lines.join("\n");
        assert!(joined.contains("cargo build"));
    }
}
