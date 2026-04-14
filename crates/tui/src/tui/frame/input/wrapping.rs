use unicode_width::UnicodeWidthChar;

use crate::theme::theme;
use crate::utils::{pad_to_width, visible_width};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct InputWrap {
    pub lines: Vec<String>,
    pub cursor_row: usize,
    pub cursor_col: usize,
}

pub(crate) fn measure_input(text: &str, cursor: usize, width: usize) -> InputWrap {
    let width = width.max(1);
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut current_width = 0usize;
    let mut cursor_row = 0usize;
    let mut cursor_col = 0usize;
    let mut row = 0usize;
    let mut col = 0usize;
    let mut seen_cursor = false;

    if cursor == 0 {
        seen_cursor = true;
        cursor_row = 0;
        cursor_col = 0;
    }

    for (byte_idx, ch) in text.char_indices() {
        if !seen_cursor && byte_idx == cursor {
            seen_cursor = true;
            cursor_row = row;
            cursor_col = col;
        }

        if ch == '\n' {
            lines.push(current.clone());
            current.clear();
            current_width = 0;
            row += 1;
            col = 0;
            continue;
        }

        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(1).max(1);
        if current_width + ch_width > width && !current.is_empty() {
            lines.push(current.clone());
            current.clear();
            current_width = 0;
            row += 1;
            col = 0;
        }

        current.push(ch);
        current_width += ch_width;
        col += ch_width;

        if current_width >= width {
            lines.push(current.clone());
            current.clear();
            current_width = 0;
            row += 1;
            col = 0;
        }
    }

    if !seen_cursor && cursor == text.len() {
        cursor_row = row;
        cursor_col = col;
    }

    lines.push(current);
    if text.ends_with('\n') {
        lines.push(String::new());
        if cursor == text.len() {
            cursor_row = row + 1;
            cursor_col = 0;
        }
    }

    InputWrap {
        lines,
        cursor_row,
        cursor_col,
    }
}

pub(crate) fn format_border_top(width: usize, lines_above: usize, border_color: &str) -> String {
    let t = theme();
    let border = if lines_above > 0 {
        let indicator = format!("─── ↑ {} more ", lines_above);
        let remaining = width.saturating_sub(visible_width(&indicator));
        format!("{}{}", indicator, "─".repeat(remaining))
    } else {
        "─".repeat(width)
    };
    format!("{border_color}{border}{}", t.reset)
}

pub(crate) fn format_border_bottom(width: usize, lines_below: usize, border_color: &str) -> String {
    let t = theme();
    let border = if lines_below > 0 {
        let indicator = format!("─── ↓ {} more ", lines_below);
        let remaining = width.saturating_sub(visible_width(&indicator));
        format!("{}{}", indicator, "─".repeat(remaining))
    } else {
        "─".repeat(width)
    };
    format!("{border_color}{border}{}", t.reset)
}

pub(crate) fn blank_line(width: usize) -> String {
    pad_to_width("", width)
}
