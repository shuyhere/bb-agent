use unicode_width::UnicodeWidthChar;

use super::{
    layout::compute_layout,
    renderer::FrameBuffer,
    state::{FullscreenState, TranscriptItem},
};

pub fn build_frame(state: &FullscreenState) -> FrameBuffer {
    let input_inner_width = state.size.width.saturating_sub(2).max(1) as usize;
    let (wrapped_input_lines, cursor_row, cursor_col) =
        wrap_input_with_cursor(&state.input, state.cursor, input_inner_width);
    let layout = compute_layout(state.size, wrapped_input_lines.len());

    let mut lines = vec![blank_line(state.size.width as usize); state.size.height as usize];

    render_transcript(
        state,
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
        wrapped_input_lines,
        cursor_row,
        cursor_col,
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

fn render_transcript(state: &FullscreenState, width: usize, height: usize) -> Vec<String> {
    if width == 0 || height == 0 {
        return Vec::new();
    }

    let mut all_lines = Vec::new();
    for item in &state.transcript {
        all_lines.extend(render_transcript_item(item, width));
        all_lines.push(blank_line(width));
    }
    while all_lines.len() < height {
        all_lines.insert(0, blank_line(width));
    }

    let total = all_lines.len();
    let clamped_scroll = state.transcript_scroll.min(total.saturating_sub(height));
    let end = total.saturating_sub(clamped_scroll);
    let start = end.saturating_sub(height);
    let mut visible = all_lines[start..end].to_vec();
    while visible.len() < height {
        visible.insert(0, blank_line(width));
    }
    visible
}

fn render_transcript_item(item: &TranscriptItem, width: usize) -> Vec<String> {
    let prefix = format!("{:<7} ", item.role.label());
    let content_width = width.saturating_sub(display_width(&prefix)).max(1);
    let wrapped = wrap_plain_text(&item.text, content_width);
    let mut lines = Vec::new();

    for (index, line) in wrapped.into_iter().enumerate() {
        let leader = if index == 0 {
            prefix.clone()
        } else {
            " ".repeat(display_width(&prefix))
        };
        lines.push(pad_to_width(
            &format!("{}{}", leader, truncate_to_width(&line, content_width)),
            width,
        ));
    }

    if lines.is_empty() {
        lines.push(pad_to_width(&prefix, width));
    }

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
        " {} fullscreen foundation • {} • size {}x{} • scroll {} ",
        spinner, state.status_line, state.size.width, state.size.height, state.transcript_scroll,
    );
    pad_to_width(&truncate_to_width(&text, width), width)
}

fn render_input(
    state: &FullscreenState,
    input_y: u16,
    width: usize,
    height: usize,
    wrapped_input_lines: Vec<String>,
    cursor_row: usize,
    cursor_col: usize,
) -> (Vec<String>, Option<(u16, u16)>) {
    if width == 0 || height == 0 {
        return (Vec::new(), None);
    }

    if height == 1 {
        return ((vec![pad_to_width("input", width)]), None);
    }

    let inner_width = width.saturating_sub(2).max(1);
    let inner_height = height.saturating_sub(2);
    let top = format_border_top(width);
    let bottom = format_border_bottom(width);

    let display_lines = if state.input.is_empty() {
        vec![state.input_placeholder.clone()]
    } else {
        wrapped_input_lines
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
    let remaining = width.saturating_sub(2 + display_width(title));
    format!("┌{}{}┐", title, "─".repeat(remaining))
}

fn format_border_bottom(width: usize) -> String {
    if width <= 2 {
        return "─".repeat(width);
    }

    let hint = " Enter submit • Shift+Enter newline ";
    let truncated_hint = truncate_to_width(hint, width.saturating_sub(2));
    let remaining = width.saturating_sub(2 + display_width(&truncated_hint));
    format!("└{}{}┘", truncated_hint, "─".repeat(remaining))
}

fn wrap_input_with_cursor(text: &str, cursor: usize, width: usize) -> (Vec<String>, usize, usize) {
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

    if !current.is_empty() || text.is_empty() {
        lines.push(current);
    }

    while lines.len() <= cursor_row {
        lines.push(String::new());
    }

    (lines, cursor_row, cursor_col)
}

fn wrap_plain_text(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines = Vec::new();

    for physical_line in text.split('\n') {
        if physical_line.is_empty() {
            lines.push(String::new());
            continue;
        }

        let mut current = String::new();
        let mut current_width = 0usize;
        for ch in physical_line.chars() {
            let ch_width = UnicodeWidthChar::width(ch).unwrap_or(1).max(1);
            if current_width + ch_width > width && !current.is_empty() {
                lines.push(current.clone());
                current.clear();
                current_width = 0;
            }

            current.push(ch);
            current_width += ch_width;

            if current_width >= width {
                lines.push(current.clone());
                current.clear();
                current_width = 0;
            }
        }

        if !current.is_empty() {
            lines.push(current);
        }
    }

    if lines.is_empty() {
        lines.push(String::new());
    }

    lines
}

fn truncate_to_width(text: &str, width: usize) -> String {
    let mut result = String::new();
    let mut used = 0usize;
    for ch in text.chars() {
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(1).max(1);
        if used + ch_width > width {
            break;
        }
        result.push(ch);
        used += ch_width;
    }
    result
}

fn pad_to_width(text: &str, width: usize) -> String {
    let text = truncate_to_width(text, width);
    let pad = width.saturating_sub(display_width(&text));
    format!("{}{}", text, " ".repeat(pad))
}

fn blank_line(width: usize) -> String {
    " ".repeat(width)
}

fn display_width(text: &str) -> usize {
    text.chars()
        .map(|ch| UnicodeWidthChar::width(ch).unwrap_or(1).max(1))
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fullscreen::{layout::Size, state::FullscreenAppConfig};

    #[test]
    fn build_frame_keeps_full_terminal_height() {
        let state = FullscreenState::new(
            FullscreenAppConfig::default(),
            Size {
                width: 40,
                height: 12,
            },
        );
        let frame = build_frame(&state);

        assert_eq!(frame.lines.len(), 12);
    }

    #[test]
    fn wrapping_tracks_cursor_after_exact_line_fill() {
        let (lines, row, col) = wrap_input_with_cursor("abcd", 4, 4);
        assert_eq!(lines[0], "abcd");
        assert_eq!(row, 1);
        assert_eq!(col, 0);
    }
}
