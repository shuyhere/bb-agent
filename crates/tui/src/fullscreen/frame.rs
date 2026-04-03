use unicode_width::UnicodeWidthChar;

use crate::utils::{pad_to_width, truncate_to_width, visible_width};

use super::{
    layout::compute_layout, projection::TranscriptProjection, renderer::FrameBuffer,
    runtime::FullscreenState,
};

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

pub(crate) fn build_frame(state: &FullscreenState) -> FrameBuffer {
    let input_inner_width = state.size.width.saturating_sub(2).max(1) as usize;
    let input_wrap = measure_input(&state.input, state.cursor, input_inner_width);
    let layout = compute_layout(state.size, input_wrap.lines.len());

    let mut lines = vec![blank_line(state.size.width as usize); state.size.height as usize];

    render_transcript(
        state,
        &state.projection,
        layout.transcript.width as usize,
        layout.transcript.height as usize,
    )
    .into_iter()
    .enumerate()
    .for_each(|(offset, line)| {
        if let Some(slot) = lines.get_mut(layout.transcript.y as usize + offset) {
            *slot = line;
        }
    });

    if layout.status.height > 0 {
        lines[layout.status.y as usize] = render_status(state, layout.status.width as usize);
    }

    let (input_lines, cursor) = render_input(
        state,
        layout.input.y,
        layout.input.width as usize,
        layout.input.height as usize,
        input_wrap,
    );
    input_lines
        .into_iter()
        .enumerate()
        .for_each(|(offset, line)| {
            if let Some(slot) = lines.get_mut(layout.input.y as usize + offset) {
                *slot = line;
            }
        });

    FrameBuffer { lines, cursor }
}

fn render_transcript(
    state: &FullscreenState,
    projection: &TranscriptProjection,
    width: usize,
    height: usize,
) -> Vec<String> {
    if width == 0 || height == 0 {
        return Vec::new();
    }

    let visible = state.viewport.visible_row_range();
    let mut lines: Vec<String> = projection.rows[visible]
        .iter()
        .map(|row| pad_to_width(&truncate_to_width(&row.text, width), width))
        .collect();

    while lines.len() < height {
        lines.insert(0, blank_line(width));
    }

    lines.truncate(height);
    lines
}

fn render_status(state: &FullscreenState, width: usize) -> String {
    let spinner = match state.tick_count % 4 {
        0 => "|",
        1 => "/",
        2 => "-",
        _ => "\\",
    };
    let text = format!(
        " {spinner} {} • size {}x{} • row {} of {} ",
        state.status_line,
        state.size.width,
        state.size.height,
        state.viewport.viewport_top,
        state.viewport.total_projected_rows,
    );
    pad_to_width(&truncate_to_width(&text, width), width)
}

fn render_input(
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

    let inner_width = width.saturating_sub(2).max(1);
    let inner_height = height.saturating_sub(2);
    let top = format_border_top(width);
    let bottom = format_border_bottom(width);

    let display_lines = if state.input.is_empty() {
        vec![state.input_placeholder.clone()]
    } else {
        wrapped_lines
    };

    let max_start = display_lines.len().saturating_sub(inner_height);
    let visible_start = cursor_row
        .saturating_sub(inner_height.saturating_sub(1))
        .min(max_start);
    let visible_end = (visible_start + inner_height).min(display_lines.len());
    let visible_slice = &display_lines[visible_start..visible_end];

    let mut lines = Vec::with_capacity(height);
    lines.push(top);

    for row in 0..inner_height {
        let content = visible_slice.get(row).map(String::as_str).unwrap_or("");
        let body = truncate_to_width(content, inner_width);
        lines.push(format!("│{}│", pad_to_width(&body, inner_width)));
    }

    lines.push(bottom);

    let cursor = if state.input.is_empty() {
        Some((1, input_y + 1))
    } else {
        let visible_cursor_row = cursor_row.saturating_sub(visible_start);
        if visible_cursor_row < inner_height {
            Some((
                (1 + cursor_col.min(inner_width)) as u16,
                input_y + 1 + visible_cursor_row as u16,
            ))
        } else {
            None
        }
    };

    (lines, cursor)
}

fn format_border_top(width: usize) -> String {
    if width <= 2 {
        return "─".repeat(width);
    }

    let title = " Input ";
    let remaining = width.saturating_sub(2 + visible_width(title));
    format!("┌{}{}┐", title, "─".repeat(remaining))
}

fn format_border_bottom(width: usize) -> String {
    if width <= 2 {
        return "─".repeat(width);
    }

    let hint = " Enter submit • Shift+Enter newline ";
    let remaining = width.saturating_sub(2 + visible_width(hint));
    format!("└{}{}┘", hint, "─".repeat(remaining))
}

fn blank_line(width: usize) -> String {
    " ".repeat(width)
}
