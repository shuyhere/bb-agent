mod attachments;
mod dialogs;
mod wrapping;

#[cfg(test)]
mod tests;

use crate::theme::theme;
use crate::utils::{pad_to_width, truncate_to_width};

use super::super::{runtime::TuiState, types::TuiMode};
use attachments::render_attachment_lines;
pub(crate) use attachments::{attachment_chip_label, attachment_line_count, visible_input_text};
pub(crate) use dialogs::{
    measure_approval_input, render_approval_dialog, render_approval_input, render_auth_dialog,
};
pub(crate) use wrapping::{
    InputWrap, blank_line, format_border_bottom, format_border_top, measure_input,
};

pub(crate) fn render_input(
    state: &TuiState,
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

    let mut display_lines = render_attachment_lines(state, inner_width);
    let attachment_rows = display_lines.len();
    if state.input.is_empty() {
        display_lines.push(format!(
            "{}{}{}",
            theme().dim,
            state.input_placeholder,
            theme().reset
        ));
    } else {
        display_lines.extend(wrapped_lines);
    }

    let absolute_cursor_row = attachment_rows + cursor_row;
    let max_start = display_lines.len().saturating_sub(inner_height);
    let visible_start = absolute_cursor_row
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

    let cursor = if state.mode != TuiMode::Normal {
        None
    } else if state.input.is_empty() {
        let visible_cursor_row = absolute_cursor_row.saturating_sub(visible_start);
        if visible_cursor_row < inner_height {
            Some((0, input_y + 1 + visible_cursor_row as u16))
        } else {
            None
        }
    } else {
        let visible_cursor_row = absolute_cursor_row.saturating_sub(visible_start);
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
