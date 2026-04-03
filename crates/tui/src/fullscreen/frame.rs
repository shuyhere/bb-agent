use unicode_width::UnicodeWidthChar;

use crate::theme::theme;
use crate::utils::{pad_to_width, truncate_to_width, visible_width};

use super::{
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
    let layout = state.current_layout();

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

    lines.extend(
        visible_rows
            .iter()
            .map(|row| render_transcript_row(state, row, width, focused_block)),
    );

    lines.truncate(height);
    while lines.len() < height {
        lines.push(blank_line(width));
    }
    lines
}

fn render_transcript_row(
    state: &FullscreenState,
    row: &super::projection::ProjectedRow,
    width: usize,
    focused_block: Option<super::transcript::BlockId>,
) -> String {
    let Some(block) = state.transcript.block(row.block_id) else {
        return blank_line(width);
    };
    let t = theme();
    let plain = transcript_row_text(state, row, block);

    if plain.trim().is_empty() {
        return match &block.kind {
            super::transcript::BlockKind::UserMessage => {
                render_boxed_ansi_line("", width, &t.user_msg_bg)
            }
            super::transcript::BlockKind::ToolUse => {
                render_boxed_ansi_line("", width, tool_use_bg(block, t))
            }
            super::transcript::BlockKind::ToolResult if !block.content.trim().is_empty() => {
                render_boxed_ansi_line("", width, tool_result_bg(block, t))
            }
            _ => blank_line(width),
        };
    }

    let padded = pad_to_width(&truncate_to_width(&plain, width), width);
    if focused_block == Some(row.block_id) && row.kind == ProjectedRowKind::Header {
        return format!("\x1b[7m{padded}\x1b[0m");
    }

    match &block.kind {
        super::transcript::BlockKind::UserMessage => {
            render_boxed_line(&plain, width, &t.user_msg_bg)
        }
        super::transcript::BlockKind::Thinking => {
            format!(
                "{}{}{}{}",
                t.italic,
                t.thinking_text,
                pad_to_width(&truncate_to_width(&plain, width), width),
                t.reset
            )
        }
        super::transcript::BlockKind::ToolUse => {
            let content = if row.kind == ProjectedRowKind::Header {
                format!("{}{}{}", t.bold, truncate_to_width(&plain, width), t.reset)
            } else {
                format!("{}{}{}", t.dim, truncate_to_width(&plain, width), t.reset)
            };
            render_boxed_ansi_line(&content, width, tool_use_bg(block, t))
        }
        super::transcript::BlockKind::ToolResult => {
            let content = if row.kind == ProjectedRowKind::Header {
                format!("{}{}{}", t.bold, truncate_to_width(&plain, width), t.reset)
            } else {
                style_tool_result_line(block, &plain, width)
            };
            render_boxed_ansi_line(&content, width, tool_result_bg(block, t))
        }
        super::transcript::BlockKind::SystemNote => render_note_line(block, &plain, width),
        super::transcript::BlockKind::AssistantMessage => {
            if row.kind == ProjectedRowKind::Content {
                format!("{}{}{}", t.text, padded, t.reset)
            } else {
                padded
            }
        },
    }
}

fn transcript_row_text(
    state: &FullscreenState,
    row: &super::projection::ProjectedRow,
    block: &super::transcript::TranscriptBlock,
) -> String {
    if !matches!(state.mode, FullscreenMode::Normal) {
        return row.text.clone();
    }

    match (row.kind, &block.kind) {
        (ProjectedRowKind::Header, super::transcript::BlockKind::UserMessage)
        | (ProjectedRowKind::Header, super::transcript::BlockKind::AssistantMessage)
        | (ProjectedRowKind::Header, super::transcript::BlockKind::Thinking)
        | (ProjectedRowKind::Header, super::transcript::BlockKind::SystemNote)
        | (ProjectedRowKind::Header, super::transcript::BlockKind::ToolResult) => String::new(),
        (ProjectedRowKind::Header, _) => strip_tree_header_prefix(&row.text),
        (ProjectedRowKind::Content, _) => row.text.trim_start().to_string(),
    }
}

fn tool_use_bg<'a>(block: &super::transcript::TranscriptBlock, t: &'a crate::theme::Theme) -> &'a str {
    if block.title.contains("error") {
        &t.tool_error_bg
    } else if block.title.contains("done") {
        &t.tool_success_bg
    } else {
        &t.tool_pending_bg
    }
}

fn tool_result_bg<'a>(block: &super::transcript::TranscriptBlock, t: &'a crate::theme::Theme) -> &'a str {
    if block.title.contains("error") {
        &t.tool_error_bg
    } else {
        &t.tool_success_bg
    }
}

fn style_tool_result_line(
    block: &super::transcript::TranscriptBlock,
    text: &str,
    width: usize,
) -> String {
    let t = theme();
    let line = truncate_to_width(text, width);
    if block.title.contains("error") {
        return format!("{}{}{}", t.error, line, t.reset);
    }

    let trimmed = crate::utils::strip_ansi(&line);
    if is_diff_added(&trimmed) {
        format!("{}{}{}", t.diff_added, line, t.reset)
    } else if is_diff_removed(&trimmed) {
        format!("{}{}{}", t.diff_removed, line, t.reset)
    } else if trimmed.starts_with("@@") || trimmed.starts_with("diff ") || trimmed.starts_with("index ") {
        format!("{}{}{}", t.diff_context, line, t.reset)
    } else if trimmed.starts_with("exit code:")
        || trimmed.starts_with("read ")
        || trimmed.starts_with("wrote ")
        || trimmed.starts_with("applied ")
        || trimmed.starts_with("details:")
        || trimmed.starts_with("artifact:")
        || trimmed.starts_with("… output truncated")
    {
        format!("{}{}{}", t.dim, line, t.reset)
    } else {
        format!("{}{}{}", t.tool_output, line, t.reset)
    }
}

fn is_diff_added(text: &str) -> bool {
    text.starts_with('+') && !text.starts_with("+++")
}

fn is_diff_removed(text: &str) -> bool {
    text.starts_with('-') && !text.starts_with("---")
}

fn strip_tree_header_prefix(text: &str) -> String {
    let mut trimmed = text.trim_start();
    for prefix in ["▸ ", "▾ ", "• "] {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            trimmed = rest;
            break;
        }
    }
    for label in ["you ", "bb ", "thinking ", "tool ", "result ", "note "] {
        if let Some(rest) = trimmed.strip_prefix(label) {
            return rest.to_string();
        }
    }
    trimmed.to_string()
}

fn render_note_line(
    block: &super::transcript::TranscriptBlock,
    text: &str,
    width: usize,
) -> String {
    let t = theme();
    let body = pad_to_width(&truncate_to_width(text, width), width);
    match block.title.as_str() {
        "error" => format!("{}{}{}", t.error, body, t.reset),
        "warning" => format!("{}{}{}{}", t.dim, t.warning, body, t.reset),
        "status" => format!("{}{}{}", t.dim, body, t.reset),
        _ => format!("{}{}{}{}", t.dim, t.yellow, body, t.reset),
    }
}

fn render_boxed_line(text: &str, width: usize, bg: &str) -> String {
    render_boxed_ansi_line(&truncate_to_width(text, width), width, bg)
}

fn render_boxed_ansi_line(content: &str, width: usize, bg: &str) -> String {
    let t = theme();
    if width <= 2 {
        let body = truncate_to_width(content, width);
        return format!("{bg}{body}{}", t.reset);
    }

    let inner_width = width.saturating_sub(2);
    let truncated = truncate_to_width(content, inner_width);
    let visible = visible_width(&truncated);
    let pad = inner_width.saturating_sub(visible);
    let content_with_bg = truncated.replace(&t.reset, &format!("{}{bg}", t.reset));
    format!("{bg} {content_with_bg}{} {}", " ".repeat(pad), t.reset)
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
                if state.status_line.trim().is_empty() {
                    format!("{spinner} Working...")
                } else {
                    format!("{spinner} {}", state.status_line)
                }
            } else {
                state.status_line.clone()
            }
        }
        FullscreenMode::Transcript => {
            let base = if state.status_line.trim().is_empty() {
                "Transcript mode".to_string()
            } else {
                state.status_line.clone()
            };
            let follow = if state.viewport.auto_follow { "follow on" } else { "follow paused" };
            format!("{base} • {follow}")
        }
        FullscreenMode::Search => {
            if state.search.query.is_empty() {
                "search /".to_string()
            } else {
                format!("search /{}", state.search.query)
            }
        }
    };
    if text.trim().is_empty() {
        return blank_line(width);
    }
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

    if let Some(menu_lines) = state.render_slash_menu_lines(width) {
        let mut lines = menu_lines;
        lines.truncate(height);
        while lines.len() < height {
            lines.push(blank_line(width));
        }
        return lines;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boxed_line_uses_full_width_background() {
        let line = render_boxed_ansi_line("hi", 20, &theme().user_msg_bg);
        assert!(!line.contains("\x1b[0m      "));
    }
}
