use std::path::{Path, PathBuf};

use crate::theme::theme;
use crate::utils::truncate_to_width;

use super::super::super::runtime::TuiState;

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

fn parse_input_attachment_at(input: &str, index: usize, cwd: &Path) -> Option<(PathBuf, usize)> {
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
        PathBuf::from(trimmed)
    } else {
        cwd.join(trimmed)
    };
    path.is_file().then_some((path, token_len))
}

fn collect_input_attachment_paths(input: &str, cwd: &Path) -> Vec<PathBuf> {
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

/// Hide inline `@file` tokens from the visible input buffer while preserving a cursor position
/// that still points at the same user-visible location. The cursor snaps to the elided gap when
/// it lands anywhere inside an attachment token.
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

pub(super) fn render_attachment_lines(state: &TuiState, width: usize) -> Vec<String> {
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
