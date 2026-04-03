use unicode_width::UnicodeWidthChar;

use crate::theme::theme;
use crate::utils::{pad_to_width, truncate_to_width, visible_width};

use super::{
    layout::compute_layout,
    projection::{ProjectedRowKind, TranscriptProjection},
    renderer::FrameBuffer,
    runtime::{FullscreenMode, FullscreenState},
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
    let input_inner_width = state.size.width.max(1) as usize;
    let input_wrap = measure_input(&state.input, state.cursor, input_inner_width);
    let layout = compute_layout(state.size, input_wrap.lines.len());

    let mut lines = vec![blank_line(state.size.width as usize); state.size.height as usize];

    render_header(state, layout.header.width as usize)
        .into_iter()
        .enumerate()
        .for_each(|(offset, line)| {
            if let Some(slot) = lines.get_mut(layout.header.y as usize + offset) {
                *slot = line;
            }
        });

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
    render_footer(state, layout.footer.width as usize, layout.footer.height as usize)
        .into_iter()
        .enumerate()
        .for_each(|(offset, line)| {
            if let Some(slot) = lines.get_mut(layout.footer.y as usize + offset) {
                *slot = line;
            }
        });
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

fn render_header(state: &FullscreenState, width: usize) -> Vec<String> {
    if width == 0 {
        return Vec::new();
    }

    let mut lines = Vec::new();
    if !state.title.is_empty() {
        lines.push(format!("\x1b[1m\x1b[36m{}\x1b[0m", pad_to_width(&truncate_to_width(&state.title, width), width)));
        let hints = "\x1b[90mCtrl-C exit . / commands . ! bash . F2 thinking . /help for more\x1b[0m";
        lines.push(pad_to_width(&truncate_to_width(hints, width), width));
        lines.push(blank_line(width));
    }
    lines
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
    let visible_rows = &projection.rows[visible.clone()];
    let focused_block = matches!(
        state.mode,
        FullscreenMode::Transcript | FullscreenMode::Search
    )
    .then_some(state.focused_block)
    .flatten();

    let mut lines = Vec::with_capacity(height);
    let top_padding = height.saturating_sub(visible_rows.len());
    for _ in 0..top_padding {
        lines.push(blank_line(width));
    }

    lines.extend(
        visible_rows
            .iter()
            .map(|row| render_transcript_row(state, row, width, focused_block)),
    );

    lines.truncate(height);
    lines
}

fn render_transcript_row(
    state: &FullscreenState,
    row: &super::projection::ProjectedRow,
    width: usize,
    focused_block: Option<super::transcript::BlockId>,
) -> String {
    let kind = state.transcript.block(row.block_id).map(|block| block.kind.clone());
    let t = theme();

    let plain = match (&state.mode, row.kind, kind.as_ref()) {
        (FullscreenMode::Normal, ProjectedRowKind::Header, Some(super::transcript::BlockKind::UserMessage)) => String::new(),
        (FullscreenMode::Normal, ProjectedRowKind::Header, Some(super::transcript::BlockKind::AssistantMessage)) => String::new(),
        (FullscreenMode::Normal, ProjectedRowKind::Header, Some(super::transcript::BlockKind::Thinking)) => String::new(),
        _ => row.text.clone(),
    };

    let body = pad_to_width(&truncate_to_width(&plain, width), width);
    if body.trim().is_empty() {
        return blank_line(width);
    }

    if focused_block == Some(row.block_id) && row.kind == ProjectedRowKind::Header {
        return format!("\x1b[7m{body}\x1b[0m");
    }

    match kind {
        Some(super::transcript::BlockKind::UserMessage) => {
            format!("{}{body}{}", t.user_msg_bg, t.reset)
        }
        Some(super::transcript::BlockKind::Thinking) => {
            format!("{}{}{body}{}", t.italic, t.thinking_text, t.reset)
        }
        Some(super::transcript::BlockKind::ToolUse) => {
            let content = if row.kind == ProjectedRowKind::Header {
                format!("{}{}{}", t.bold, body, t.reset)
            } else {
                body
            };
            format!("{}{content}{}", t.tool_pending_bg, t.reset)
        }
        Some(super::transcript::BlockKind::ToolResult) => {
            format!("{}{body}{}", t.tool_success_bg, t.reset)
        }
        Some(super::transcript::BlockKind::SystemNote) => {
            format!("{}{}{}{}", t.dim, t.yellow, body, t.reset)
        }
        _ => body,
    }
}

fn render_status(state: &FullscreenState, width: usize) -> String {
    let t = theme();
    let spinner = match state.tick_count % 4 {
        0 => "|",
        1 => "/",
        2 => "-",
        _ => "\\",
    };
    let text = match state.mode {
        FullscreenMode::Normal => {
            if state.has_active_turn() {
                format!("{spinner} {}", state.status_line)
            } else {
                state.status_line.clone()
            }
        }
        FullscreenMode::Transcript => {
            let follow = if state.viewport.auto_follow { "follow on" } else { "follow paused" };
            format!("{} • {}", state.status_line, follow)
        }
        FullscreenMode::Search => {
            if state.search.query.is_empty() {
                "search /".to_string()
            } else {
                format!("search /{}", state.search.query)
            }
        }
    };
    format!(
        "{}{}{}",
        t.dim,
        pad_to_width(&truncate_to_width(&text, width), width),
        t.reset
    )
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

    let inner_width = width.max(1);
    let inner_height = height.saturating_sub(2);

    let display_lines = if state.input.is_empty() {
        vec![format!("{}{}{}", theme().dim, state.input_placeholder, theme().reset)]
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

    let mut lines = Vec::with_capacity(height);
    lines.push(format_border_top(width, visible_start));

    for row in 0..inner_height {
        let content = visible_slice.get(row).map(String::as_str).unwrap_or("");
        let body = truncate_to_width(content, inner_width);
        lines.push(pad_to_width(&body, inner_width));
    }

    lines.push(format_border_bottom(width, lines_below));

    let cursor = if state.mode != FullscreenMode::Normal {
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

fn render_footer(state: &FullscreenState, width: usize, height: usize) -> Vec<String> {
    if width == 0 || height == 0 {
        return Vec::new();
    }

    let t = theme();
    let mut lines = Vec::with_capacity(height);
    let line1 = if state.footer.line1.is_empty() {
        String::new()
    } else {
        format!("{}{}{}", t.dim, truncate_to_width(&state.footer.line1, width), t.reset)
    };
    lines.push(pad_to_width(&line1, width));

    let second = if state.footer.line2_right.is_empty() {
        state.footer.line2_left.clone()
    } else {
        let left = truncate_to_width(&state.footer.line2_left, width);
        let right = truncate_to_width(&state.footer.line2_right, width);
        let used = visible_width(&left) + visible_width(&right);
        if used + 2 <= width {
            let gap = " ".repeat(width - used);
            format!("{left}{gap}{right}")
        } else {
            truncate_to_width(&format!("{left}  {right}"), width)
        }
    };
    if height > 1 {
        lines.push(format!("{}{}{}", t.dim, pad_to_width(&second, width), t.reset));
    }
    lines.truncate(height);
    while lines.len() < height {
        lines.push(blank_line(width));
    }
    lines
}

fn format_border_top(width: usize, lines_above: usize) -> String {
    let t = theme();
    let border = if lines_above > 0 {
        let indicator = format!("─── ↑ {} more ", lines_above);
        let remaining = width.saturating_sub(visible_width(&indicator));
        format!("{}{}", indicator, "─".repeat(remaining))
    } else {
        "─".repeat(width)
    };
    format!("{}{}{}", t.border_accent, border, t.reset)
}

fn format_border_bottom(width: usize, lines_below: usize) -> String {
    let t = theme();
    let border = if lines_below > 0 {
        let indicator = format!("─── ↓ {} more ", lines_below);
        let remaining = width.saturating_sub(visible_width(&indicator));
        format!("{}{}", indicator, "─".repeat(remaining))
    } else {
        "─".repeat(width)
    };
    format!("{}{}{}", t.border_accent, border, t.reset)
}

fn blank_line(width: usize) -> String {
    " ".repeat(width)
}
