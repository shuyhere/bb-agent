use crate::theme::theme;
use crate::utils::{ansi_sequence_len, char_width, pad_to_width, strip_ansi, truncate_to_width};

use super::super::{
    projection::{ProjectedRow, ProjectedRowKind, TranscriptProjection},
    runtime::FullscreenState,
    transcript::{BlockId, BlockKind, TranscriptBlock},
    types::FullscreenMode,
};
use super::input::blank_line;

pub(crate) fn render_transcript(
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
    let focused_block = matches!(state.mode, FullscreenMode::Transcript)
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
    row: &ProjectedRow,
    width: usize,
    focused_block: Option<BlockId>,
) -> String {
    if row.kind == ProjectedRowKind::Spacer {
        return blank_line(width);
    }

    let Some(block) = state.transcript.block(row.block_id) else {
        return blank_line(width);
    };
    let t = theme();
    let plain = transcript_row_text(row, block);
    if plain.trim().is_empty() {
        return blank_line(width);
    }

    let content = match &block.kind {
        BlockKind::UserMessage => style_user_line(&plain, width),
        BlockKind::Thinking => style_thinking_line(&plain, width),
        BlockKind::ToolUse => {
            if row.kind == ProjectedRowKind::Header {
                style_tool_header(state, block, &plain, width)
            } else {
                style_tool_use_line(&plain, width)
            }
        }
        BlockKind::ToolResult => style_tool_result_line(block, &plain, width),
        BlockKind::SystemNote => {
            if row.kind == ProjectedRowKind::Header
                && matches!(block.title.as_str(), "branch summary" | "compaction")
            {
                render_summary_header_line(block, &plain, width)
            } else {
                render_note_line(block, &plain, width)
            }
        }
        BlockKind::AssistantMessage => {
            format!(
                "{}{}{}",
                t.text,
                pad_to_width(&truncate_to_width(&plain, width), width),
                t.reset
            )
        }
    };

    let visible = pad_to_width(&truncate_to_width(&content, width), width);
    let focused = focused_block == Some(row.block_id) && row.kind == ProjectedRowKind::Header;

    if focused {
        if matches!(block.kind, BlockKind::ToolUse) {
            render_focused_tool_header_line(&visible, block)
        } else {
            format!("\x1b[7m{}\x1b[0m", truncate_to_width(&visible, width))
        }
    } else if let Some((start, end)) = state.selection_span_for_row(row.index) {
        apply_selection_highlight(&visible, start, end)
    } else {
        visible
    }
}

fn apply_selection_highlight(text: &str, start: usize, end: usize) -> String {
    if start >= end {
        return text.to_string();
    }

    let mut result = String::new();
    let mut col = 0usize;
    let mut i = 0usize;
    let bytes = text.as_bytes();
    let mut in_selection = false;
    let mut active_sgr = String::new();

    while i < bytes.len() {
        if bytes[i] == 0x1b
            && let Some(len) = ansi_sequence_len(bytes, i)
        {
            let seq = &text[i..i + len];
            result.push_str(seq);
            if is_sgr_sequence(seq) {
                if is_reset_sgr(seq) {
                    active_sgr.clear();
                } else {
                    active_sgr.push_str(seq);
                }
                if in_selection {
                    result.push_str("\x1b[7m");
                }
            }
            i += len;
            continue;
        }

        let Some(ch) = text[i..].chars().next() else {
            break;
        };
        let cw = char_width(ch);
        let next_col = col + cw;
        let char_selected = col < end && next_col > start;

        if char_selected && !in_selection {
            result.push_str("\x1b[7m");
            in_selection = true;
        } else if !char_selected && in_selection {
            result.push_str("\x1b[0m");
            result.push_str(&active_sgr);
            in_selection = false;
        }

        result.push(ch);
        col = next_col;
        i += ch.len_utf8();
    }

    if in_selection {
        result.push_str("\x1b[0m");
    }

    result
}

fn is_sgr_sequence(seq: &str) -> bool {
    seq.starts_with("\x1b[") && seq.ends_with('m')
}

fn is_reset_sgr(seq: &str) -> bool {
    seq == "\x1b[0m" || seq == "\x1b[m"
}

fn transcript_row_text(row: &ProjectedRow, block: &TranscriptBlock) -> String {
    match (row.kind, &block.kind) {
        (ProjectedRowKind::Spacer, _) => String::new(),
        (ProjectedRowKind::Header, BlockKind::UserMessage)
        | (ProjectedRowKind::Header, BlockKind::AssistantMessage)
        | (ProjectedRowKind::Header, BlockKind::Thinking)
        | (ProjectedRowKind::Header, BlockKind::ToolResult) => String::new(),
        (ProjectedRowKind::Header, BlockKind::SystemNote)
            if matches!(block.title.as_str(), "branch summary" | "compaction") =>
        {
            row.text.clone()
        }
        (ProjectedRowKind::Header, BlockKind::SystemNote) => String::new(),
        (ProjectedRowKind::Header, _) => row.text.clone(),
        (ProjectedRowKind::Content, _) => row.text.clone(),
    }
}

fn style_user_line(text: &str, width: usize) -> String {
    let t = theme();
    let line = truncate_to_width(text, width);
    if let Some(rest) = line.strip_prefix("❯ ") {
        format!("{}{}❯{} {}{}", t.bold, t.accent, t.reset, t.text, rest)
    } else {
        format!("{}{}{}", t.text, line, t.reset)
    }
}

fn style_thinking_line(text: &str, width: usize) -> String {
    let t = theme();
    let line = truncate_to_width(text, width);
    format!("{}{}{}{}", t.italic, t.thinking_text, line, t.reset)
}

fn tool_status_color<'a>(block: &TranscriptBlock, t: &'a crate::theme::Theme) -> &'a str {
    if block.title.contains("error") {
        &t.error
    } else if block.title.contains("done") {
        &t.success
    } else if block.title.contains("running") {
        &t.accent
    } else {
        &t.dim
    }
}

const RUNNING_DOT_BLINK_MS: u64 = 500;

fn tool_status_dot(state: &FullscreenState, block: &TranscriptBlock) -> &'static str {
    if block.title.contains("running") {
        if ((state.tick_count * 80) / RUNNING_DOT_BLINK_MS).is_multiple_of(2) {
            "●"
        } else {
            "·"
        }
    } else {
        "●"
    }
}

fn focused_tool_header_bg<'a>(block: &TranscriptBlock, t: &'a crate::theme::Theme) -> &'a str {
    if block.title.contains("error") {
        &t.tool_error_bg
    } else if block.title.contains("done") {
        &t.tool_success_bg
    } else {
        &t.tool_pending_bg
    }
}

fn render_focused_tool_header_line(content: &str, block: &TranscriptBlock) -> String {
    let t = theme();
    let plain = strip_ansi(content);
    let plain = plain
        .trim_start_matches('▌')
        .trim_start_matches('▎')
        .trim_start();
    let marked = if let Some(rest) = plain.strip_prefix("● ") {
        format!("▶ {rest}")
    } else if let Some(rest) = plain.strip_prefix("· ") {
        format!("▶ {rest}")
    } else if plain.starts_with('▶') {
        plain.to_string()
    } else {
        format!("▶ {plain}")
    };

    if !t.colors_enabled() {
        return format!("\x1b[7m{}\x1b[0m", marked);
    }

    let bg = focused_tool_header_bg(block, t);
    format!("\x1b[7m{bg}{}{}{}", t.bold, marked, t.reset)
}

fn style_tool_header(
    state: &FullscreenState,
    block: &TranscriptBlock,
    text: &str,
    width: usize,
) -> String {
    let t = theme();
    let line = truncate_to_width(text, width);
    let status = tool_status_color(block, t);

    if let Some(rest) = line.strip_prefix("● ") {
        if let Some((name, args)) = rest.split_once('(') {
            let args = format!("({args}");
            let display_name = display_tool_header_name(name.trim());
            format!(
                "{}{}{} {}{}{}{}{}{args}{}",
                status,
                tool_status_dot(state, block),
                t.reset,
                t.bold,
                status,
                display_name,
                t.reset,
                t.muted,
                t.reset,
            )
        } else {
            let display_name = display_tool_header_name(rest.trim());
            format!(
                "{}{}{} {}{}{}",
                status,
                tool_status_dot(state, block),
                t.reset,
                t.bold,
                status,
                display_name,
            ) + &t.reset
        }
    } else {
        format!("{}{}{}", t.bold, line, t.reset)
    }
}

fn display_tool_header_name(name: &str) -> String {
    match name {
        "bash" => "Bash".to_string(),
        "read" => "Read".to_string(),
        "write" => "Write".to_string(),
        "edit" => "Edit".to_string(),
        "ls" => "LS".to_string(),
        "grep" => "Grep".to_string(),
        "find" => "Find".to_string(),
        other => other.to_string(),
    }
}

fn style_tool_use_line(text: &str, width: usize) -> String {
    style_response_line(text, width, false)
}

fn style_tool_result_line(block: &TranscriptBlock, text: &str, width: usize) -> String {
    let is_error = block.title.contains("error");
    style_response_line(text, width, is_error)
}

fn is_diff_line(stripped: &str) -> bool {
    if !stripped.starts_with("    ") {
        return false;
    }
    let after = &stripped[4..];
    if after.is_empty() {
        return false;
    }
    let first = after.as_bytes()[0];
    match first {
        b'-' | b'+' => after[1..]
            .trim_start()
            .starts_with(|c: char| c.is_ascii_digit()),
        b' ' => {
            after[1..]
                .trim_start()
                .starts_with(|c: char| c.is_ascii_digit())
                || after.trim() == "..."
        }
        _ => false,
    }
}

fn pad_diff_line_bg(line: &str, width: usize) -> String {
    let stripped = crate::utils::strip_ansi(line);
    let after_indent = stripped.get(4..).unwrap_or("");
    let first_byte = after_indent.as_bytes().first().copied().unwrap_or(0);
    let is_changed = first_byte == b'-' || first_byte == b'+';
    if !is_changed {
        return line.to_string();
    }
    let vis_w = crate::utils::visible_width(line);
    if vis_w >= width {
        return line.to_string();
    }
    let padding = " ".repeat(width - vis_w);
    if let Some(pos) = line.rfind("\x1b[0m") {
        let mut result = String::with_capacity(line.len() + padding.len());
        result.push_str(&line[..pos]);
        result.push_str(&padding);
        result.push_str(&line[pos..]);
        result
    } else {
        format!("{}{}", line, padding)
    }
}

fn style_response_line(text: &str, width: usize, is_error: bool) -> String {
    let t = theme();
    let line = truncate_to_width(text, width);
    let trimmed = crate::utils::strip_ansi(&line);

    if is_diff_line(&trimmed) {
        return pad_diff_line_bg(&line, width);
    }

    if let Some(rest) = line.strip_prefix("  ⎿  ") {
        let body_color = if is_error { &t.error } else { &t.text };
        return format!("{}  ⎿  {}{}{}", t.dim, t.reset, body_color, rest) + &t.reset;
    }
    if let Some(rest) = line.strip_prefix("     ") {
        let body_color = if is_error { &t.error } else { &t.text };
        return format!("     {}{}{}", body_color, rest, t.reset);
    }
    if trimmed.contains("(Ctrl+Shift+O tool expand)") {
        return format!("{}{}{}", t.dim, line, t.reset);
    }
    if is_error {
        format!("{}{}{}", t.error, line, t.reset)
    } else if trimmed.starts_with("exit code:")
        || trimmed.starts_with("read ")
        || trimmed.starts_with("wrote ")
        || trimmed.starts_with("applied ")
        || trimmed.starts_with("details:")
        || trimmed.starts_with("artifact:")
        || trimmed.starts_with("... (")
        || trimmed.starts_with("… output truncated")
        || trimmed == "executing..."
    {
        format!("{}{}{}", t.dim, line, t.reset)
    } else {
        format!("{}{}{}", t.text, line, t.reset)
    }
}

fn render_note_line(block: &TranscriptBlock, text: &str, width: usize) -> String {
    let t = theme();
    let body = pad_to_width(&truncate_to_width(text, width), width);
    match block.title.as_str() {
        "branch summary" => {
            let accent = if text.starts_with('│') || text.starts_with('╰') {
                &t.custom_msg_label
            } else {
                &t.muted
            };
            format!("{}{}{}{}", t.custom_msg_bg, accent, body, t.reset)
        }
        "compaction" => {
            let accent = if text.starts_with('│') || text.starts_with('╰') {
                &t.warning
            } else {
                &t.muted
            };
            format!("{}{}{}{}", t.info_bg, accent, body, t.reset)
        }
        "error" => format!("{}{}{}{}", t.tool_error_bg, t.error, body, t.reset),
        "warning" => format!("{}{}{}{}", t.info_bg, t.warning, body, t.reset),
        "status" => {
            if text.starts_with("[Skills]") {
                format!("{}{}{}{}", t.bold, t.custom_msg_label, body, t.reset)
            } else if text.starts_with("  /skill:") {
                format!("{}{}{}{}", t.bold, t.accent, body, t.reset)
            } else if text.starts_with("[Prompts]") {
                format!("{}{}{}{}", t.bold, t.border_accent, body, t.reset)
            } else if text.starts_with("[Extensions]") {
                format!("{}{}{}{}", t.bold, t.success, body, t.reset)
            } else if text.starts_with("    ") {
                format!("{}{}{}", t.muted, body, t.reset)
            } else if text.starts_with("  /") {
                format!("{}{}{}", t.border_accent, body, t.reset)
            } else if text.starts_with("  ") {
                format!("{}{}{}", t.success, body, t.reset)
            } else {
                format!("{}{}{}", t.text, body, t.reset)
            }
        }
        _ => format!("{}{}{}", t.custom_msg_label, body, t.reset),
    }
}

fn render_summary_header_line(block: &TranscriptBlock, text: &str, width: usize) -> String {
    let t = theme();
    let boxed = format!("╭─ {text}");
    let body = pad_to_width(&truncate_to_width(&boxed, width), width);
    match block.title.as_str() {
        "branch summary" => format!(
            "{}{}{}{}{}",
            t.custom_msg_bg, t.bold, t.custom_msg_label, body, t.reset
        ),
        "compaction" => format!("{}{}{}{}{}", t.info_bg, t.bold, t.warning, body, t.reset),
        _ => format!("{}{}{}", t.bold, body, t.reset),
    }
}
