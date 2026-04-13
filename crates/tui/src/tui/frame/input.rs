use std::path::Path;

use unicode_width::UnicodeWidthChar;

use crate::theme::theme;
use crate::utils::{pad_to_width, truncate_to_width, visible_width, word_wrap};

use super::super::runtime::TuiState;

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

pub(crate) fn attachment_chip_label(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    let size_kb = std::fs::metadata(path).ok()?.len().div_ceil(1024);
    Some(format!("[{name}, {size_kb}KB]"))
}

fn attachment_chip_line(path: &Path, width: usize) -> Option<String> {
    let t = theme();
    Some(truncate_to_width(
        &format!("{}{}{}", t.accent, attachment_chip_label(path)?, t.reset),
        width,
    ))
}

fn parse_input_attachment_at(
    input: &str,
    index: usize,
    cwd: &Path,
) -> Option<(std::path::PathBuf, usize)> {
    let rest = input.get(index + 1..)?;

    let (raw_path, token_len) = if let Some(quoted) = rest.strip_prefix('"') {
        let end = quoted.find('"')?;
        (&quoted[..end], end + 3)
    } else {
        let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        (&rest[..end], end + 1)
    };

    if raw_path.is_empty() {
        return None;
    }

    let trimmed = raw_path.trim_end_matches([',', '.', ';', ':', ')', ']', '}']);
    let path = if Path::new(trimmed).is_absolute() {
        std::path::PathBuf::from(trimmed)
    } else {
        cwd.join(trimmed)
    };
    path.is_file().then_some((path, token_len))
}

fn collect_input_attachment_paths(input: &str, cwd: &Path) -> Vec<std::path::PathBuf> {
    let mut paths = Vec::new();
    let bytes = input.as_bytes();
    let mut index = 0usize;

    while index < bytes.len() {
        if bytes[index] != b'@' {
            index += 1;
            continue;
        }

        if let Some((path, consumed)) = parse_input_attachment_at(input, index, cwd) {
            paths.push(path);
            index += consumed;
        } else {
            index += 1;
        }
    }

    paths
}

pub(crate) fn visible_input_text(input: &str, cursor: usize, cwd: &Path) -> (String, usize) {
    let bytes = input.as_bytes();
    let mut out = String::new();
    let mut index = 0usize;
    let mut visible_cursor = None;

    while index < bytes.len() {
        if visible_cursor.is_none() && cursor == index {
            visible_cursor = Some(out.len());
        }

        if bytes[index] == b'@'
            && let Some((_path, consumed)) = parse_input_attachment_at(input, index, cwd)
        {
            let token_end = index + consumed;
            if visible_cursor.is_none() && cursor > index && cursor <= token_end {
                visible_cursor = Some(out.len());
            }
            index = token_end;
            while index < bytes.len() {
                let Some(ch) = input[index..].chars().next() else {
                    break;
                };
                if ch != ' ' && ch != '\t' {
                    break;
                }
                if visible_cursor.is_none() && cursor > index && cursor <= index + ch.len_utf8() {
                    visible_cursor = Some(out.len());
                }
                index += ch.len_utf8();
                if !out.is_empty() && !out.ends_with(char::is_whitespace) {
                    out.push(' ');
                }
            }
            continue;
        }

        let Some(ch) = input[index..].chars().next() else {
            break;
        };
        out.push(ch);
        index += ch.len_utf8();
    }

    let out_len = out.len();
    if visible_cursor.is_none() {
        visible_cursor = Some(out_len);
    }

    (out, visible_cursor.unwrap_or(out_len))
}

pub(crate) fn attachment_line_count(state: &TuiState, width: usize) -> usize {
    render_attachment_lines(state, width).len()
}

fn render_attachment_lines(state: &TuiState, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for path in &state.pending_image_paths {
        let path = Path::new(path);
        if seen.insert(path.to_path_buf())
            && let Some(line) = attachment_chip_line(path, width)
        {
            lines.push(line);
        }
    }

    for path in collect_input_attachment_paths(&state.input, &state.cwd) {
        if seen.insert(path.clone())
            && let Some(line) = attachment_chip_line(&path, width)
        {
            lines.push(line);
        }
    }

    lines
}

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

    let cursor = if state.mode != super::super::types::TuiMode::Normal {
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
type OverlayLines = Vec<(usize, String)>;

fn approval_options(
    dialog: &super::super::types::TuiApprovalDialog,
) -> Vec<(super::super::types::TuiApprovalChoice, String)> {
    let mut options = vec![(
        super::super::types::TuiApprovalChoice::ApproveOnce,
        "Yes, proceed [y]".to_string(),
    )];
    if dialog.allow_session {
        let scope = dialog
            .session_scope_label
            .as_deref()
            .unwrap_or("this command");
        options.push((
            super::super::types::TuiApprovalChoice::ApproveForSession,
            format!("Yes, and don't ask again for {scope} in this session [a]"),
        ));
    }
    options.push((
        super::super::types::TuiApprovalChoice::Deny,
        "No, and tell BB what to do differently [n]".to_string(),
    ));
    options
}

fn push_wrapped_line(lines: &mut Vec<String>, line: &str, width: usize) {
    if line.is_empty() {
        lines.push(String::new());
    } else {
        lines.extend(word_wrap(line, width));
    }
}

fn render_auth_step(step: &super::super::types::TuiAuthStep, width: usize) -> Vec<String> {
    let t = theme();
    let prefix = match step.state {
        Some(super::super::types::TuiAuthStepState::Done) => {
            format!("{}✓{} ", t.success, t.reset)
        }
        Some(super::super::types::TuiAuthStepState::Active) => {
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

fn render_clickable_url_segment(url: &str, segment: &str) -> String {
    let t = theme();
    format!(
        "\x1b]8;;{url}\x07{}{}{}\x1b]8;;\x07",
        t.md_link, segment, t.reset
    )
}

fn wrap_clickable_url(url: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let chars = url.chars().collect::<Vec<_>>();
    let mut lines = Vec::new();
    let mut start = 0usize;

    while start < chars.len() {
        let end = (start + width).min(chars.len());
        let segment = chars[start..end].iter().collect::<String>();
        lines.push(render_clickable_url_segment(url, &segment));
        start = end;
    }

    if lines.is_empty() {
        lines.push(render_clickable_url_segment(url, url));
    }

    lines
}

fn render_dialog_row(width: usize, start_x: usize, row: String) -> String {
    pad_to_width(&format!("{}{}", " ".repeat(start_x), row), width)
}

pub(crate) fn render_auth_dialog(
    state: &TuiState,
    width: usize,
    height: usize,
) -> Option<AuthDialogRender> {
    let dialog = state.auth_dialog.as_ref()?;
    if width < 20 || height < 8 {
        return None;
    }

    let t = theme();
    let panel_width = width.clamp(20, 90);
    let inner_width = panel_width.saturating_sub(2);

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
        content.extend(wrap_clickable_url(url, inner_width));
        content.push(format!(
            "{}Alt+C or F6 copies this URL to your clipboard{}",
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
        "Esc to cancel • Alt+C / F6 copy URL"
    } else {
        "Esc to cancel"
    };
    content.push(format!("{}{}{}", t.dim, shortcuts, t.reset));

    let panel_height = content.len().min(height.saturating_sub(2));
    let start_y = (height.saturating_sub(panel_height)) / 2;
    let start_x = (width.saturating_sub(panel_width)) / 2;

    let header_keep = usize::from(panel_height > 0);
    let body_height = panel_height.saturating_sub(header_keep);
    let body_len = content.len().saturating_sub(header_keep);
    let max_body_start = body_len.saturating_sub(body_height);
    let body_input_row = input_row_in_content.and_then(|row| row.checked_sub(header_keep));
    let body_start = if content.len() > panel_height {
        if let Some(input_row) = body_input_row {
            input_row
                .saturating_add(2)
                .saturating_sub(body_height)
                .min(max_body_start)
        } else {
            0
        }
    } else {
        0
    };

    let mut visible_content = Vec::new();
    if header_keep == 1 {
        visible_content.push(content[0].clone());
    }
    visible_content.extend(
        content
            .iter()
            .skip(header_keep + body_start)
            .take(body_height)
            .cloned(),
    );

    let mut rendered = Vec::new();
    for row in 0..panel_height {
        let content_line = visible_content
            .get(row)
            .map(|s| truncate_to_width(s, inner_width))
            .unwrap_or_default();
        rendered.push((
            start_y + row,
            render_dialog_row(width, start_x, content_line),
        ));
    }

    let cursor = input_row_in_content.and_then(|row_index| {
        let visible_row = if row_index == 0 {
            Some(0)
        } else {
            row_index
                .checked_sub(header_keep + body_start)
                .map(|row| row + header_keep)
        }?;
        if visible_row >= panel_height {
            return None;
        }
        let input_text = if state.input.is_empty() {
            String::new()
        } else {
            state.input.clone()
        };
        let input_row = start_y + visible_row;
        let display = truncate_to_width(&format!("{input_prefix}{input_text}"), inner_width);
        let input_col = start_x + visible_width(&display);
        Some((
            input_col.min(width.saturating_sub(1)) as u16,
            input_row.min(height.saturating_sub(1)) as u16,
        ))
    });

    Some((rendered, cursor))
}

pub(crate) fn measure_approval_input(
    dialog: &super::super::types::TuiApprovalDialog,
    width: usize,
) -> usize {
    approval_input_content(dialog, width.max(1)).0.len()
}

fn approval_input_content(
    dialog: &super::super::types::TuiApprovalDialog,
    width: usize,
) -> (Vec<String>, Option<(usize, String, String)>) {
    let t = theme();
    let mut content = Vec::new();
    let mut cursor = None;

    content.push(truncate_to_width(
        &format!("{}{}{}{}", t.bold, t.accent, dialog.title, t.reset),
        width,
    ));
    content.push(truncate_to_width(
        &format!("{}$ {}{}", t.accent, dialog.command, t.reset),
        width,
    ));
    content.push(truncate_to_width(
        &format!("{}Reason:{} {}", t.dim, t.reset, dialog.reason),
        width,
    ));
    for line in &dialog.lines {
        content.push(truncate_to_width(line, width));
    }
    for (choice, label) in approval_options(dialog) {
        let rendered = if dialog.selected == choice {
            format!("{}→ {}{}", t.accent, label, t.reset)
        } else {
            format!("  {}", label)
        };
        content.push(truncate_to_width(&rendered, width));
    }

    if dialog.selected == super::super::types::TuiApprovalChoice::Deny {
        let prefix = "    steer: ".to_string();
        let placeholder = dialog
            .deny_input_placeholder
            .as_deref()
            .unwrap_or("Tell BB what to do differently");
        let input_value = if dialog.deny_input.is_empty() {
            format!("{}{}{}", t.muted, placeholder, t.reset)
        } else {
            dialog.deny_input.clone()
        };
        cursor = Some((content.len(), prefix.clone(), dialog.deny_input.clone()));
        content.push(truncate_to_width(&format!("{prefix}{input_value}"), width));
    }

    let footer = if dialog.selected == super::super::types::TuiApprovalChoice::Deny {
        format!(
            "{}Type feedback • Enter deny • Tab/↑/↓ move • Esc deny now{}",
            t.dim, t.reset
        )
    } else {
        format!(
            "{}y once • a session • n deny • Tab/↑/↓ move • Enter confirm{}",
            t.dim, t.reset
        )
    };
    content.push(truncate_to_width(&footer, width));
    (content, cursor)
}

pub(crate) fn render_approval_input(
    state: &TuiState,
    input_y: u16,
    width: usize,
    height: usize,
) -> (Vec<String>, Option<(u16, u16)>) {
    let Some(dialog) = state.approval_dialog.as_ref() else {
        return (Vec::new(), None);
    };
    if width == 0 || height == 0 {
        return (Vec::new(), None);
    }

    let inner_height = height.saturating_sub(2);
    let (content, approval_cursor) = approval_input_content(dialog, width.max(1));
    let visible_start = 0;
    let visible_end = (visible_start + inner_height).min(content.len());
    let visible_slice = &content[visible_start..visible_end];
    let lines_above = visible_start;
    let lines_below = content.len().saturating_sub(visible_end);
    let border_color = state.color_theme.border_escape();

    let mut lines = Vec::with_capacity(height);
    lines.push(format_border_top(width, lines_above, &border_color));
    for row in 0..inner_height {
        let content = visible_slice.get(row).map(String::as_str).unwrap_or("");
        lines.push(pad_to_width(&truncate_to_width(content, width), width));
    }
    lines.push(format_border_bottom(width, lines_below, &border_color));

    let cursor = approval_cursor.and_then(|(row, prefix, input_text)| {
        if row < visible_start || row >= visible_end {
            return None;
        }
        let visible_row = row - visible_start;
        let display = truncate_to_width(&format!("{prefix}{input_text}"), width);
        Some((
            visible_width(&display).min(width.saturating_sub(1)) as u16,
            input_y + 1 + visible_row as u16,
        ))
    });

    (lines, cursor)
}

pub(crate) fn render_approval_dialog(
    _state: &TuiState,
    _width: usize,
    _height: usize,
) -> Option<OverlayLines> {
    None
}
