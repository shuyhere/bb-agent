use crate::theme::theme;
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle, ThemeSet};
use syntect::parsing::SyntaxSet;

mod blocks;
mod cache;
mod github;
mod text;

use blocks::{
    has_markdown_syntax, markdown_block_tokens, render_plain_text, stable_token_count,
    trim_blank_edges,
};
use cache::{get_cached_render, put_cached_render, text_hash};
#[cfg(test)]
use github::auto_link_github_refs_with_repo;
use github::{auto_link_github_refs, create_hyperlink};
use text::{strip_ansi, visible_width, word_wrap_ansi};

// ANSI escape codes
const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const ITALIC: &str = "\x1b[3m";
const UNDERLINE: &str = "\x1b[4m";
const STRIKETHROUGH: &str = "\x1b[9m";
static HEADING_H1: std::sync::LazyLock<&'static str> = std::sync::LazyLock::new(|| {
    Box::leak(format!("{}{}{}", theme().md_heading, BOLD, UNDERLINE).into_boxed_str())
});
static HEADING_H2: std::sync::LazyLock<&'static str> = std::sync::LazyLock::new(|| {
    Box::leak(format!("{}{}", theme().md_heading, BOLD).into_boxed_str())
});
static HEADING_H3: std::sync::LazyLock<&'static str> = std::sync::LazyLock::new(|| {
    Box::leak(format!("{}{}", theme().border_accent, BOLD).into_boxed_str())
});
static HEADING_DEFAULT: std::sync::LazyLock<&'static str> =
    std::sync::LazyLock::new(|| Box::leak(format!("{}{}", theme().accent, BOLD).into_boxed_str()));
const CODE_INLINE: &str = "\x1b[38;5;75m";
const QUOTE_PREFIX: &str = "\x1b[90m│\x1b[0m ";
const BULLET: &str = "\x1b[90m•\x1b[0m";
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
        if let Some((cached_width, ref lines)) = self.cached_lines
            && cached_width == width
        {
            return lines.clone();
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
    // In markdown table
    in_table: bool,
    in_table_head: bool,
    table_rows: Vec<(bool, Vec<String>)>,
    table_row: Vec<String>,
    table_cell: String,
    // Block quote depth
    quote_depth: usize,
    // List stack: None = unordered, Some(n) = ordered starting at n
    list_stack: Vec<Option<u64>>,
    // Current item index in ordered list
    list_counters: Vec<u64>,
    // Link state
    link_url: Option<String>,
    link_start_len: Option<usize>,
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
            in_table: false,
            in_table_head: false,
            table_rows: Vec::new(),
            table_row: Vec::new(),
            table_cell: String::new(),
            quote_depth: 0,
            list_stack: Vec::new(),
            list_counters: Vec::new(),
            link_url: None,
            link_start_len: None,
            in_paragraph: false,
            syntax_set: SyntaxSet::load_defaults_newlines(),
            theme_set: ThemeSet::load_defaults(),
        }
    }

    fn push_style(&mut self, code: &'static str) {
        self.style_stack.push(code);
        if self.in_table {
            self.table_cell.push_str(code);
        } else {
            self.current_line.push_str(code);
        }
    }

    fn pop_style(&mut self) {
        self.style_stack.pop();
        if self.in_table {
            self.table_cell.push_str(RESET);
            for &style in &self.style_stack {
                self.table_cell.push_str(style);
            }
        } else {
            self.current_line.push_str(RESET);
            for &style in &self.style_stack {
                self.current_line.push_str(style);
            }
        }
    }

    fn active_styles(&self) -> String {
        self.style_stack.iter().copied().collect()
    }

    fn line_prefix(&self) -> String {
        let mut prefix = String::new();
        for _ in 0..self.quote_depth {
            prefix.push_str(QUOTE_PREFIX);
        }
        prefix
    }

    fn prefix_width(&self) -> usize {
        self.quote_depth * 2
    }

    fn list_indent(&self) -> String {
        "  ".repeat(self.list_stack.len().saturating_sub(1))
    }

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

    fn push_blank(&mut self) {
        if self
            .lines
            .last()
            .is_some_and(|last| !strip_ansi(last).trim().is_empty())
        {
            let prefix = self.line_prefix();
            self.lines.push(prefix);
        }
    }

    fn push_text(&mut self, text: &str) {
        if self.in_table {
            self.table_cell.push_str(text);
        } else {
            self.current_line.push_str(text);
        }
    }

    fn flush_table_cell(&mut self) {
        let cell = self.table_cell.trim().to_string();
        self.table_row.push(cell);
        self.table_cell.clear();
    }

    fn flush_table_row(&mut self) {
        if self.table_row.is_empty() {
            return;
        }
        self.table_rows
            .push((self.in_table_head, std::mem::take(&mut self.table_row)));
    }

    fn render_table(&mut self) {
        if self.table_rows.is_empty() {
            return;
        }

        let prefix = self.line_prefix();
        let prefix_w = self.prefix_width();
        let avail = (self.width as usize).saturating_sub(prefix_w).max(1);
        let column_count = self
            .table_rows
            .iter()
            .map(|(_, row)| row.len())
            .max()
            .unwrap_or(0);
        if column_count == 0 {
            self.table_rows.clear();
            return;
        }

        let separator_width = if column_count > 0 {
            (column_count.saturating_sub(1)) * 3 + 4
        } else {
            0
        };
        let min_col_width = 3usize;
        let mut widths = vec![min_col_width; column_count];
        for (_, row) in &self.table_rows {
            for (index, cell) in row.iter().enumerate() {
                widths[index] = widths[index].max(visible_width(cell));
            }
        }

        let max_content_width = avail.saturating_sub(separator_width).max(column_count);
        while widths.iter().sum::<usize>() > max_content_width {
            if let Some((idx, _)) = widths.iter().enumerate().max_by_key(|(_, width)| **width) {
                if widths[idx] <= min_col_width {
                    break;
                }
                widths[idx] -= 1;
            } else {
                break;
            }
        }

        let top_border = widths
            .iter()
            .map(|width| "─".repeat(*width + 2))
            .collect::<Vec<_>>()
            .join("┬");
        self.lines
            .push(format!("{prefix}{DIM}┌{top_border}┐{RESET}"));

        let rows = std::mem::take(&mut self.table_rows);
        for (is_header, row) in rows {
            let wrapped_cells = (0..column_count)
                .map(|index| {
                    let cell = row.get(index).map(String::as_str).unwrap_or("");
                    let mut wrapped = word_wrap_ansi(cell, widths[index]);
                    if wrapped.is_empty() {
                        wrapped.push(String::new());
                    }
                    wrapped
                })
                .collect::<Vec<_>>();
            let row_height = wrapped_cells.iter().map(Vec::len).max().unwrap_or(1);

            for line_index in 0..row_height {
                let mut line = format!("{prefix}{DIM}│{RESET} ");
                for col in 0..column_count {
                    if col > 0 {
                        line.push_str(&format!(" {DIM}│{RESET} "));
                    }
                    let cell_line = wrapped_cells[col]
                        .get(line_index)
                        .cloned()
                        .unwrap_or_default();
                    let pad = widths[col].saturating_sub(visible_width(&cell_line));
                    if is_header {
                        line.push_str(BOLD);
                    }
                    line.push_str(&cell_line);
                    line.push_str(&" ".repeat(pad));
                    if is_header {
                        line.push_str(RESET);
                    }
                }
                line.push_str(&format!(" {DIM}│{RESET}"));
                self.lines.push(line);
            }

            if is_header {
                let separator = widths
                    .iter()
                    .map(|width| "─".repeat(*width + 2))
                    .collect::<Vec<_>>()
                    .join("┼");
                self.lines
                    .push(format!("{prefix}{DIM}├{separator}┤{RESET}"));
            }
        }

        let bottom_border = widths
            .iter()
            .map(|width| "─".repeat(*width + 2))
            .collect::<Vec<_>>()
            .join("┴");
        self.lines
            .push(format!("{prefix}{DIM}└{bottom_border}┘{RESET}"));
    }

    fn render_code_block(&mut self) {
        let lang = self.code_block.take().unwrap_or_default();
        let code_lines = std::mem::take(&mut self.code_block_lines);
        let prefix = self.line_prefix();
        let prefix_w = self.prefix_width();
        let avail = (self.width as usize).saturating_sub(prefix_w).max(1);

        let opening_fence = if lang.is_empty() {
            "```".to_string()
        } else {
            format!("```{lang}")
        };
        self.lines
            .push(format!("{}{DIM}{}{RESET}", prefix, opening_fence));

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

            let wrapped = word_wrap_ansi(&styled, avail);
            if wrapped.is_empty() {
                self.lines.push(prefix.clone());
            } else {
                for line in wrapped {
                    self.lines.push(format!("{}{}", prefix, line));
                }
            }
        }

        self.lines.push(format!("{}{DIM}```{RESET}", prefix));
    }
}

fn render_markdown(text: &str, width: u16) -> Vec<String> {
    if !has_markdown_syntax(text) {
        return render_plain_text(text, width);
    }
    render_markdown_streaming(text, width)
}

fn render_markdown_streaming(text: &str, width: u16) -> Vec<String> {
    let tokens = markdown_block_tokens(text);
    if tokens.is_empty() {
        return vec![String::new()];
    }

    let stable_count = stable_token_count(text, &tokens);
    let mut lines: Vec<String> = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        if index > 0
            && !lines
                .last()
                .is_some_and(|line| strip_ansi(line).trim().is_empty())
        {
            lines.push(String::new());
        }

        let rendered = if index < stable_count {
            render_markdown_block_cached(&token.text, width)
        } else {
            render_markdown_block(&token.text, width)
        };
        lines.extend(trim_blank_edges(rendered));
    }

    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

fn render_markdown_block_cached(text: &str, width: u16) -> Vec<String> {
    let key = (text_hash(text), width);
    if let Some(lines) = get_cached_render(key) {
        return lines;
    }

    let lines = render_markdown_block(text, width);
    put_cached_render(key, lines.clone());
    lines
}

fn render_markdown_block(text: &str, width: u16) -> Vec<String> {
    let mut opts = Options::empty();
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
                } else if state.in_table {
                    state.table_cell.push(' ');
                } else {
                    state.push_text(" ");
                }
            }
            Event::HardBreak => {
                if state.in_table {
                    state.table_cell.push(' ');
                } else {
                    state.flush_line();
                }
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
                HeadingLevel::H1 => *HEADING_H1,
                HeadingLevel::H2 => *HEADING_H2,
                HeadingLevel::H3 => *HEADING_H3,
                _ => *HEADING_DEFAULT,
            };
            state.push_style(style);
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
            state.push_style(ITALIC);
        }
        Tag::Table(_) => {
            state.flush_line();
            state.push_blank();
            state.in_table = true;
            state.in_table_head = false;
            state.table_rows.clear();
            state.table_row.clear();
            state.table_cell.clear();
            state.in_paragraph = false;
        }
        Tag::TableHead => {
            state.in_table_head = true;
        }
        Tag::TableRow => {
            state.table_row.clear();
            state.table_cell.clear();
        }
        Tag::TableCell => {
            state.table_cell.clear();
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
                .map(|stack| stack.is_some())
                .unwrap_or(false);

            if is_ordered {
                let counter_val = if let Some(counter) = state.list_counters.last_mut() {
                    let value = *counter;
                    *counter += 1;
                    value
                } else {
                    1
                };
                let depth = state.list_stack.len().saturating_sub(1);
                state.push_text(&format!(
                    "{}{} ",
                    indent,
                    ordered_list_marker(depth, counter_val)
                ));
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
            state.link_start_len = Some(current_text_target(state).len());
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
            state.pop_style();
            state.flush_line();
            state.quote_depth = state.quote_depth.saturating_sub(1);
        }
        TagEnd::Table => {
            if !state.table_cell.is_empty() {
                state.flush_table_cell();
            }
            if !state.table_row.is_empty() {
                state.flush_table_row();
            }
            state.render_table();
            state.in_table = false;
            state.in_table_head = false;
            state.table_row.clear();
            state.table_cell.clear();
            state.push_blank();
        }
        TagEnd::TableHead => {
            if !state.table_cell.is_empty() {
                state.flush_table_cell();
            }
            if !state.table_row.is_empty() {
                state.flush_table_row();
            }
            state.in_table_head = false;
        }
        TagEnd::TableRow => {
            state.flush_table_row();
        }
        TagEnd::TableCell => {
            state.flush_table_cell();
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
        TagEnd::Strong | TagEnd::Emphasis | TagEnd::Strikethrough => {
            state.pop_style();
        }
        TagEnd::Link => {
            if let (Some(url), Some(start)) = (state.link_url.take(), state.link_start_len.take()) {
                let target = current_text_target_mut(state);
                if start <= target.len() {
                    let raw_link_text = target[start..].to_string();
                    target.truncate(start);
                    target.push_str(&create_hyperlink(&url, &raw_link_text));
                }
            }
        }
        _ => {}
    }
}

fn handle_text(state: &mut RenderState, text: &str) {
    if state.code_block.is_some() {
        for line in text.split('\n') {
            state.code_block_lines.push(line.to_string());
        }
        if text.ends_with('\n') && !state.code_block_lines.is_empty() {
            state.code_block_lines.pop();
        }
    } else if state.link_url.is_some() {
        state.push_text(text);
    } else {
        state.push_text(&auto_link_github_refs(text));
    }
}

fn handle_inline_code(state: &mut RenderState, code: &str) {
    state.push_text(&format!("{}`{}`{}", CODE_INLINE, code, RESET));
    let styles = state.active_styles();
    if !styles.is_empty() {
        state.push_text(&styles);
    }
}

fn current_text_target(state: &RenderState) -> &String {
    if state.in_table {
        &state.table_cell
    } else {
        &state.current_line
    }
}

fn current_text_target_mut(state: &mut RenderState) -> &mut String {
    if state.in_table {
        &mut state.table_cell
    } else {
        &mut state.current_line
    }
}

fn ordered_list_marker(depth: usize, counter: u64) -> String {
    match depth {
        0 | 1 => format!("{counter}."),
        2 => {
            let index = counter.saturating_sub(1) as u8;
            let ch = (b'a' + (index % 26)) as char;
            format!("{ch}.")
        }
        _ => format!("{}.", to_roman(counter)),
    }
}

fn to_roman(mut value: u64) -> String {
    let numerals = [
        (1000, "m"),
        (900, "cm"),
        (500, "d"),
        (400, "cd"),
        (100, "c"),
        (90, "xc"),
        (50, "l"),
        (40, "xl"),
        (10, "x"),
        (9, "ix"),
        (5, "v"),
        (4, "iv"),
        (1, "i"),
    ];
    let mut out = String::new();
    for (amount, glyph) in numerals {
        while value >= amount {
            value -= amount;
            out.push_str(glyph);
        }
    }
    out
}

#[cfg(test)]
mod tests;
