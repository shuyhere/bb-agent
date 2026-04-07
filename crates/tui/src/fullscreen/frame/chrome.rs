use crate::theme::theme;
use crate::utils::{pad_to_width, truncate_to_width, visible_width};

use super::super::{runtime::FullscreenState, types::FullscreenMode};
use super::input::blank_line;

pub(crate) fn render_header(state: &FullscreenState, width: usize) -> Vec<String> {
    if width == 0 {
        return Vec::new();
    }

    let t = theme();
    let title_color = state.color_theme.title_escape();

    let mut lines = Vec::new();
    if !state.title.is_empty() {
        lines.push(format!(
            "{}{}{}{}",
            t.bold,
            title_color,
            pad_to_width(&truncate_to_width(&state.title, width), width),
            t.reset,
        ));
        let hints = format!(
            "{}Ctrl-C exit . / commands . ! bash . click or use Ctrl+Shift+O to enter tool expand mode . Ctrl+Y drag-copy . /help for more{}",
            t.dim, t.reset
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
            if state.has_active_turn() || state.has_running_tool() {
                let msg = if state.status_line.trim().is_empty() {
                    ""
                } else {
                    &state.status_line
                };
                let rendered = state.spinner.render(msg, width);
                return pad_to_width(&rendered, width);
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
            let follow = if state.viewport.auto_follow {
                "follow on"
            } else {
                "follow paused"
            };
            format!("{base} • {follow}")
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
        format!(
            "{}{}{}",
            t.dim,
            truncate_to_width(&state.footer.line1, width),
            t.reset
        )
    };
    lines.push(pad_to_width(&line1, width));

    let second = if state.footer.line2_right.is_empty() {
        style_footer_left(&state.footer.line2_left)
    } else {
        let left_plain = truncate_to_width(&state.footer.line2_left, width);
        let right_plain = truncate_to_width(&state.footer.line2_right, width);
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
    format!("{}{}{}", t.dim, text, t.reset)
}
