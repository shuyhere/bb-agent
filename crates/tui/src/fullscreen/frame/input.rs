use unicode_width::UnicodeWidthChar;

use crate::theme::theme;
use crate::utils::{pad_to_width, truncate_to_width, visible_width, word_wrap};

use super::super::runtime::FullscreenState;

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

pub(crate) fn render_input(
    state: &FullscreenState,
    input_y: u16,
    width: usize,
    height: usize,
    input_wrap: InputWrap,
) -> (Vec<String>, Option<(u16, u16)>) {
    if width == 0 || height == 0 {
        return (Vec::new(), None);
    }

    if height == 1 {
        return (vec![pad_to_width("input", width)], None);
    }

    let InputWrap {
        lines: wrapped_lines,
        cursor_row,
        cursor_col,
    } = input_wrap;

    let inner_width = width.max(1);
    let inner_height = height.saturating_sub(2);

    let display_lines = if state.input.is_empty() {
        vec![format!(
            "{}{}{}",
            theme().dim,
            state.input_placeholder,
            theme().reset
        )]
    } else {
        wrapped_lines
    };

    let max_start = display_lines.len().saturating_sub(inner_height);
    let visible_start = cursor_row
        .saturating_sub(inner_height.saturating_sub(1))
        .min(max_start);
    let visible_end = (visible_start + inner_height).min(display_lines.len());
    let visible_slice = &display_lines[visible_start..visible_end];
    let lines_below = display_lines.len().saturating_sub(visible_end);

    let border_color = state.color_theme.border_escape();
    let mut lines = Vec::with_capacity(height);
    lines.push(format_border_top(width, visible_start, &border_color));

    for row in 0..inner_height {
        let content = visible_slice.get(row).map(String::as_str).unwrap_or("");
        let body = truncate_to_width(content, inner_width);
        lines.push(pad_to_width(&body, inner_width));
    }

    lines.push(format_border_bottom(width, lines_below, &border_color));

    let cursor = if state.mode != super::super::types::FullscreenMode::Normal {
        None
    } else if state.input.is_empty() {
        Some((0, input_y + 1))
    } else {
        let visible_cursor_row = cursor_row.saturating_sub(visible_start);
        if visible_cursor_row < inner_height {
            Some((
                cursor_col.min(inner_width) as u16,
                input_y + 1 + visible_cursor_row as u16,
            ))
        } else {
            None
        }
    };

    (lines, cursor)
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
    " ".repeat(width)
}

type AuthDialogRender = (Vec<(usize, String)>, Option<(u16, u16)>);

fn push_wrapped_line(lines: &mut Vec<String>, line: &str, width: usize) {
    if line.is_empty() {
        lines.push(String::new());
    } else {
        lines.extend(word_wrap(line, width));
    }
}

fn render_auth_step(step: &super::super::types::FullscreenAuthStep, width: usize) -> Vec<String> {
    let t = theme();
    let prefix = match step.state {
        Some(super::super::types::FullscreenAuthStepState::Done) => {
            format!("{}✓{} ", t.success, t.reset)
        }
        Some(super::super::types::FullscreenAuthStepState::Active) => {
            format!("{}●{} ", t.accent, t.reset)
        }
        _ => format!("{}○{} ", t.dim, t.reset),
    };

    let wrapped = word_wrap(&step.label, width.saturating_sub(2).max(1));
    wrapped
        .into_iter()
        .enumerate()
        .map(|(index, line)| {
            if index == 0 {
                format!("{prefix}{line}")
            } else {
                format!("  {line}")
            }
        })
        .collect()
}

fn render_clickable_url(url: &str) -> String {
    let t = theme();
    format!(
        "\x1b]8;;{url}\x07{}\x1b[4m{}{}\x1b]8;;\x07",
        t.md_link, url, t.reset
    )
}

fn render_dialog_row(width: usize, start_x: usize, row: String) -> String {
    pad_to_width(&format!("{}{}", " ".repeat(start_x), row), width)
}

pub(crate) fn render_auth_dialog(
    state: &FullscreenState,
    width: usize,
    height: usize,
) -> Option<AuthDialogRender> {
    let dialog = state.auth_dialog.as_ref()?;
    if width < 20 || height < 8 {
        return None;
    }

    let t = theme();
    let box_width = width.clamp(20, 90);
    let inner_width = box_width.saturating_sub(4);

    let mut content = Vec::new();
    let mut input_row_in_content = None;
    let input_prefix = "› ";

    content.push(format!("{}{}{}", t.bold, dialog.title, t.reset));

    if let Some(status) = &dialog.status {
        content.push(String::new());
        push_wrapped_line(
            &mut content,
            &format!("{}{}● {}{}", t.accent, t.bold, status, t.reset),
            inner_width,
        );
    }

    if !dialog.steps.is_empty() {
        content.push(String::new());
        for step in &dialog.steps {
            content.extend(render_auth_step(step, inner_width));
        }
    }

    if let Some(url) = &dialog.url {
        content.push(String::new());
        content.push(format!(
            "{}Authorization URL (click if supported){}",
            t.dim, t.reset
        ));
        content.push(render_clickable_url(url));
        content.push(format!(
            "{}Alt+C copies this URL to your clipboard{}",
            t.dim, t.reset
        ));
    }

    if !dialog.lines.is_empty() {
        content.push(String::new());
        for line in &dialog.lines {
            push_wrapped_line(&mut content, line, inner_width);
        }
    }

    if let Some(label) = &dialog.input_label {
        content.push(String::new());
        content.push(format!("{}{}{}", t.dim, label, t.reset));
        let placeholder = dialog
            .input_placeholder
            .as_deref()
            .unwrap_or("Paste here...");
        let input_value = if state.input.is_empty() {
            format!("{}{}{}", t.muted, placeholder, t.reset)
        } else {
            state.input.clone()
        };
        content.push(format!("{}{}{}", t.accent, input_prefix, input_value));
        input_row_in_content = Some(content.len() - 1);
    }

    content.push(String::new());
    let shortcuts = if dialog.url.is_some() {
        "Esc to cancel • Alt+C copy URL"
    } else {
        "Esc to cancel"
    };
    content.push(format!("{}{}{}", t.dim, shortcuts, t.reset));

    let box_height = content
        .len()
        .saturating_add(2)
        .min(height.saturating_sub(2));
    let start_y = (height.saturating_sub(box_height)) / 2;
    let start_x = (width.saturating_sub(box_width)) / 2;

    let mut rendered = Vec::new();
    let border = "─".repeat(box_width.saturating_sub(2));
    rendered.push((
        start_y,
        render_dialog_row(
            width,
            start_x,
            format!("{}┌{}┐{}", t.border_accent, border, t.reset),
        ),
    ));
    for row in 0..box_height.saturating_sub(2) {
        let content_line = content
            .get(row)
            .map(|s| truncate_to_width(s, inner_width))
            .unwrap_or_default();
        let padded = pad_to_width(&content_line, inner_width);
        rendered.push((
            start_y + 1 + row,
            render_dialog_row(
                width,
                start_x,
                format!(
                    "{}│{}{}{}│{}",
                    t.border_accent, t.reset, padded, t.border_accent, t.reset
                ),
            ),
        ));
    }
    rendered.push((
        start_y + box_height.saturating_sub(1),
        render_dialog_row(
            width,
            start_x,
            format!("{}└{}┘{}", t.border_accent, border, t.reset),
        ),
    ));

    let cursor = input_row_in_content.map(|row_index| {
        let input_text = if state.input.is_empty() {
            String::new()
        } else {
            state.input.clone()
        };
        let input_row = start_y + 1 + row_index;
        let display = truncate_to_width(&format!("{input_prefix}{input_text}"), inner_width);
        let input_col = start_x + 2 + visible_width(&display);
        (
            input_col.min(width.saturating_sub(1)) as u16,
            input_row.min(height.saturating_sub(1)) as u16,
        )
    });

    Some((rendered, cursor))
}
