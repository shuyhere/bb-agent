use crate::theme::theme;
use crate::ui_hints::TUI_HEADER_TOOL_HINT;
use crate::utils::{pad_to_width, sanitize_terminal_text, truncate_to_width, visible_width};

use super::super::{runtime::FullscreenState, types::FullscreenMode};
use super::input::blank_line;

pub(crate) fn render_header(state: &FullscreenState, width: usize) -> Vec<String> {
    if width == 0 {
        return Vec::new();
    }

    let t = theme();
    let title_color = state.color_theme.title_escape();
    let safe_title = sanitize_terminal_text(&state.title);

    let mut lines = Vec::new();
    if !safe_title.is_empty() {
        lines.push(format!(
            "{}{}{}{}",
            t.bold,
            title_color,
            pad_to_width(&truncate_to_width(&safe_title, width), width),
            t.reset,
        ));
        let hints = format!(
            "{}Ctrl-C exit . / commands . ! bash . {} . Ctrl+Y drag-copy . /help for more{}",
            t.dim, TUI_HEADER_TOOL_HINT, t.reset
        );
        lines.push(pad_to_width(&truncate_to_width(&hints, width), width));
        lines.push(blank_line(width));
    }
    lines
}

pub(crate) fn render_status(state: &FullscreenState, width: usize) -> String {
    let t = theme();
    let text = match state.mode {
        FullscreenMode::Normal => {
            if state.local_action_active && !state.queued_submission_previews.is_empty() {
                let preview = sanitize_terminal_text(
                    state
                        .queued_submission_previews
                        .back()
                        .map(String::as_str)
                        .unwrap_or_default(),
                )
                .replace('\n', " ⏎ ");
                let prefix = if state.editing_queued_messages {
                    "Editing queued"
                } else {
                    "Steering"
                };
                return format!(
                    "{}{}{}",
                    t.dim,
                    pad_to_width(
                        &truncate_to_width(
                            &format!("{prefix}: {preview} • Alt+↑ edit queued"),
                            width
                        ),
                        width
                    ),
                    t.reset
                );
            }
            if state.has_active_turn() || state.has_running_tool() {
                let msg_owned;
                let msg = if let Some(status) = state.active_turn_status_message() {
                    msg_owned = sanitize_terminal_text(&status);
                    &msg_owned
                } else if state.status_line.trim().is_empty() {
                    ""
                } else {
                    msg_owned = sanitize_terminal_text(&state.status_line);
                    &msg_owned
                };
                let rendered = state.spinner.render(msg, width);
                return pad_to_width(&rendered, width);
            } else if state.local_action_active {
                let msg_owned;
                let msg = if let Some(status) = state.local_action_status_message() {
                    msg_owned = sanitize_terminal_text(&status);
                    &msg_owned
                } else if state.status_line.trim().is_empty() {
                    ""
                } else {
                    msg_owned = sanitize_terminal_text(&state.status_line);
                    &msg_owned
                };
                let rendered = state.spinner.render(msg, width);
                return pad_to_width(&rendered, width);
            } else {
                format_plain_status_line(&state.status_line)
            }
        }
        FullscreenMode::Transcript => {
            if state.has_active_turn() || state.has_running_tool() {
                let msg_owned;
                let msg = if let Some(status) = state.active_turn_status_message() {
                    msg_owned = sanitize_terminal_text(&status);
                    &msg_owned
                } else if state.status_line.trim().is_empty() {
                    ""
                } else {
                    msg_owned = sanitize_terminal_text(&state.status_line);
                    &msg_owned
                };
                let rendered = state.spinner.render(msg, width);
                return pad_to_width(&rendered, width);
            } else if state.local_action_active {
                let msg_owned;
                let msg = if let Some(status) = state.local_action_status_message() {
                    msg_owned = sanitize_terminal_text(&status);
                    &msg_owned
                } else if state.status_line.trim().is_empty() {
                    ""
                } else {
                    msg_owned = sanitize_terminal_text(&state.status_line);
                    &msg_owned
                };
                let rendered = state.spinner.render(msg, width);
                return pad_to_width(&rendered, width);
            }

            let follow = if state.viewport.auto_follow {
                "follow on"
            } else {
                "follow paused"
            };
            if state.status_line.trim().is_empty() {
                format!("Transcript mode • {follow}")
            } else if state.status_line.contains("transcript row ") {
                state.status_line.clone()
            } else {
                format!(
                    "{} • {follow}",
                    format_plain_status_line(&state.status_line)
                )
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

pub(crate) fn render_status_lines(
    state: &FullscreenState,
    width: usize,
    height: usize,
) -> Vec<String> {
    if height == 0 {
        return Vec::new();
    }

    let status = render_status(state, width);
    if height == 1 {
        return vec![status];
    }

    let mut lines = vec![blank_line(width); height];
    let status_row = height - 1;
    lines[status_row] = status;
    lines
}

pub(crate) fn render_footer(state: &FullscreenState, width: usize, height: usize) -> Vec<String> {
    if width == 0 || height == 0 {
        return Vec::new();
    }

    if let Some(menu_lines) = state.render_tree_menu_lines(width) {
        return pad_footer_lines(menu_lines, width, height);
    }

    if let Some(menu_lines) = state.render_select_menu_lines(width) {
        return pad_footer_lines(menu_lines, width, height);
    }

    if let Some(menu_lines) = state.render_slash_menu_lines(width) {
        return pad_footer_lines(menu_lines, width, height);
    }

    if let Some(menu_lines) = state.render_at_file_menu_lines(width) {
        return pad_footer_lines(menu_lines, width, height);
    }

    let t = theme();
    let mut lines = Vec::with_capacity(height);
    let line1 = if state.footer.line1.is_empty() {
        String::new()
    } else {
        let safe_line1 = sanitize_terminal_text(&state.footer.line1);
        format!(
            "{}{}{}",
            t.dim,
            truncate_to_width(&safe_line1, width),
            t.reset
        )
    };
    lines.push(pad_to_width(&line1, width));

    let second = if state.footer.line2_right.is_empty() {
        style_footer_left(&sanitize_terminal_text(&state.footer.line2_left))
    } else {
        let left_plain =
            truncate_to_width(&sanitize_terminal_text(&state.footer.line2_left), width);
        let right_plain =
            truncate_to_width(&sanitize_terminal_text(&state.footer.line2_right), width);
        let used = visible_width(&left_plain) + visible_width(&right_plain);
        if used + 2 <= width {
            let gap = " ".repeat(width - used);
            format!(
                "{}{}{}",
                style_footer_left(&left_plain),
                gap,
                style_footer_right(&right_plain)
            )
        } else {
            style_footer_left(&truncate_to_width(
                &format!("{left_plain}  {right_plain}"),
                width,
            ))
        }
    };
    if height > 1 {
        lines.push(pad_to_width(&second, width));
    }
    lines.truncate(height);
    while lines.len() < height {
        lines.push(blank_line(width));
    }
    lines
}

fn pad_footer_lines(mut lines: Vec<String>, width: usize, height: usize) -> Vec<String> {
    lines.truncate(height);
    while lines.len() < height {
        lines.push(blank_line(width));
    }
    lines
}

fn format_plain_status_line(text: &str) -> String {
    let trimmed = sanitize_terminal_text(text).trim().to_string();
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.contains("Ctrl+") || trimmed.contains('•') || trimmed.starts_with("Transcript mode")
    {
        trimmed
    } else {
        format!("· {trimmed}")
    }
}

fn style_footer_left(text: &str) -> String {
    let t = theme();
    let Some(marker_idx) = text.find("%/") else {
        return format!("{}{}{}", t.dim, text, t.reset);
    };

    let start = text[..marker_idx]
        .rfind(' ')
        .map(|idx| idx + 1)
        .unwrap_or(0);
    let end = text[start..]
        .find(" (")
        .map(|idx| start + idx)
        .unwrap_or(text.len());
    let prefix = &text[..start];
    let token = &text[start..end];
    let suffix = &text[end..];
    let color = token
        .split('%')
        .next()
        .and_then(|value| value.parse::<f64>().ok())
        .map(|percent| {
            if percent > 90.0 {
                t.red.as_str()
            } else if percent > 70.0 {
                t.yellow.as_str()
            } else {
                ""
            }
        })
        .unwrap_or("");

    if color.is_empty() {
        format!("{}{}{}", t.dim, text, t.reset)
    } else {
        format!(
            "{}{}{}{}{}{}{}",
            t.dim, prefix, color, token, t.reset, t.dim, suffix
        ) + &t.reset
    }
}

fn style_footer_right(text: &str) -> String {
    let t = theme();
    if let Some(idx) = text.rfind("mode ") {
        let prefix = &text[..idx];
        let badge = &text[idx..];
        let badge_color = if badge.contains("yolo/") {
            t.accent.as_str()
        } else {
            t.warning.as_str()
        };
        format!(
            "{}{}{}{}{}{}",
            t.dim, prefix, badge_color, t.bold, badge, t.reset
        )
    } else {
        format!("{}{}{}", t.dim, text, t.reset)
    }
}
