use crate::theme::theme;
use crate::utils::{pad_to_width, truncate_to_width, visible_width, word_wrap};

use super::super::super::{runtime::TuiState, types};
use super::wrapping::{format_border_bottom, format_border_top};

type AuthDialogRender = (Vec<(usize, String)>, Option<(u16, u16)>);
type OverlayLines = Vec<(usize, String)>;

fn approval_options(dialog: &types::TuiApprovalDialog) -> Vec<(types::TuiApprovalChoice, String)> {
    let mut options = vec![(
        types::TuiApprovalChoice::ApproveOnce,
        "Yes, proceed [y]".to_string(),
    )];
    if dialog.allow_session {
        let scope = dialog
            .session_scope_label
            .as_deref()
            .unwrap_or("this command");
        options.push((
            types::TuiApprovalChoice::ApproveForSession,
            format!("Yes, and don't ask again for {scope} in this session [a]"),
        ));
    }
    options.push((
        types::TuiApprovalChoice::Deny,
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

fn render_auth_step(step: &types::TuiAuthStep, width: usize) -> Vec<String> {
    let t = theme();
    let prefix = match step.state {
        Some(types::TuiAuthStepState::Done) => {
            format!("{}✓{} ", t.success, t.reset)
        }
        Some(types::TuiAuthStepState::Active) => {
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

pub(crate) fn measure_approval_input(dialog: &types::TuiApprovalDialog, width: usize) -> usize {
    approval_input_content(dialog, width.max(1)).0.len()
}

fn approval_input_content(
    dialog: &types::TuiApprovalDialog,
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

    if dialog.selected == types::TuiApprovalChoice::Deny {
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

    let footer = if dialog.selected == types::TuiApprovalChoice::Deny {
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
