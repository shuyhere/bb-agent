use std::borrow::Cow;

use crate::markdown::MarkdownRenderer;
use crate::ui_hints::TOOL_EXPAND_HINT;
use crate::utils::{ansi_sequence_len, char_width, sanitize_terminal_text, visible_width};

use super::super::transcript::{BlockKind, TranscriptBlock};

const COMPACT_CONTEXT_PREVIEW_LINES: usize = 5;
const COMPACT_CONTEXT_HEADER: &str = "[Compact Context]";
const COMPACT_CONTEXT_EXPAND_HINT: &str = TOOL_EXPAND_HINT;
const LEGACY_TOOL_EXPAND_HINT: &str = "click or use Ctrl+Shift+O to enter tool expand mode";
const RESET_SGR: &str = "\x1b[0m";

pub(super) fn render_header_lines(
    block: &TranscriptBlock,
    width: usize,
    _depth: usize,
) -> Vec<String> {
    let compat = crate::theme::compatibility_mode_enabled();
    if block.kind == BlockKind::ToolUse {
        let header_text = if let Some((base, _status)) = block.title.rsplit_once(" • ") {
            base.trim().to_string()
        } else if block.title.trim().is_empty() {
            "Tool".to_string()
        } else {
            block.title.trim().to_string()
        };
        return wrap_with_prefix(&header_text, width, if compat { "* " } else { "● " }, "  ");
    }

    if is_summary_note(block) {
        let header_text = match block.title.as_str() {
            "branch summary" => {
                if compat {
                    "Branch Summary"
                } else {
                    "◆ Branch Summary"
                }
            }
            "compaction" => COMPACT_CONTEXT_HEADER,
            other => other,
        };
        return wrap_with_prefix(header_text, width, "", "");
    }

    Vec::new()
}

pub(super) fn render_collapsed_content_lines(
    block: &TranscriptBlock,
    width: usize,
    depth: usize,
) -> Vec<String> {
    if is_compaction_note(block) {
        let safe_content = sanitize_terminal_text(&block.content);
        return render_compaction_preview_lines(&safe_content, width, depth);
    }

    Vec::new()
}

pub(super) fn render_content_lines(
    block: &TranscriptBlock,
    width: usize,
    depth: usize,
) -> Vec<String> {
    let safe_content = sanitize_terminal_text(&block.content);
    if safe_content.trim().is_empty() {
        return if is_summary_note(block) {
            vec![if crate::theme::compatibility_mode_enabled() {
                "`- ".to_string()
            } else {
                "╰─ ".to_string()
            }]
        } else {
            Vec::new()
        };
    }

    match block.kind {
        BlockKind::UserMessage => wrap_with_prefix(&safe_content, width, "❯ ", "  "),
        BlockKind::AssistantMessage => render_markdown_content_lines(&safe_content, width, "", ""),
        BlockKind::Thinking => render_markdown_content_lines(&safe_content, width, "", ""),
        BlockKind::SystemNote => {
            if block.title == "compaction" {
                render_compaction_content_lines(&safe_content, width)
            } else if is_summary_note(block) {
                render_summary_block_content(&safe_content, width)
            } else {
                wrap_with_prefix(&safe_content, width, "", "")
            }
        }
        BlockKind::ToolUse => {
            if safe_content.trim_start().starts_with("```") {
                wrap_with_prefix(&safe_content, width, "", "")
            } else {
                let (first_prefix, continuation_prefix) = response_prefixes(depth, &safe_content);
                wrap_with_prefix(&safe_content, width, first_prefix, continuation_prefix)
            }
        }
        BlockKind::ToolResult => render_tool_result_content_lines(&safe_content, width, depth),
    }
}

fn is_summary_note(block: &TranscriptBlock) -> bool {
    block.kind == BlockKind::SystemNote
        && matches!(block.title.as_str(), "branch summary" | "compaction")
}

fn is_compaction_note(block: &TranscriptBlock) -> bool {
    block.kind == BlockKind::SystemNote && block.title == "compaction"
}

fn render_summary_block_content(text: &str, width: usize) -> Vec<String> {
    let mut lines = wrap_with_prefix(text, width, "│  ", "│  ");
    lines.push(if crate::theme::compatibility_mode_enabled() {
        "`- ".to_string()
    } else {
        "╰─ ".to_string()
    });
    lines
}

fn render_compaction_content_lines(text: &str, width: usize) -> Vec<String> {
    let display = compaction_display_text(text);
    if display.trim().is_empty() {
        return Vec::new();
    }
    wrap_with_prefix(display.as_ref(), width, "", "")
}

fn render_compaction_preview_lines(text: &str, width: usize, _depth: usize) -> Vec<String> {
    let display = compaction_display_text(text);
    let preview = display
        .lines()
        .take(COMPACT_CONTEXT_PREVIEW_LINES)
        .collect::<Vec<_>>()
        .join("\n");

    let mut out = Vec::new();
    if !preview.trim().is_empty() {
        out.extend(wrap_with_prefix(&preview, width, "", ""));
        out.push(String::new());
    }
    out.extend(wrap_with_prefix(COMPACT_CONTEXT_EXPAND_HINT, width, "", ""));
    out
}

fn compaction_display_text(text: &str) -> Cow<'_, str> {
    if let Some((first_line, rest)) = text.split_once('\n')
        && first_line.starts_with("[compaction:")
    {
        return Cow::Owned(rest.trim_start_matches('\n').to_string());
    }

    Cow::Borrowed(text)
}

fn render_markdown_content_lines(
    text: &str,
    width: usize,
    first_prefix: &str,
    continuation_prefix: &str,
) -> Vec<String> {
    let available_width = width
        .saturating_sub(visible_width(first_prefix).max(visible_width(continuation_prefix)))
        .max(1);
    let mut renderer = MarkdownRenderer::new(text);
    let mut first = true;
    renderer
        .render(available_width as u16)
        .into_iter()
        .map(|line| {
            let prefix = if first {
                first = false;
                first_prefix
            } else {
                continuation_prefix
            };
            if line.is_empty() {
                prefix.to_string()
            } else {
                format!("{prefix}{line}")
            }
        })
        .collect()
}

fn contains_expand_hint_text(content: &str) -> bool {
    content.contains(TOOL_EXPAND_HINT) || content.contains(LEGACY_TOOL_EXPAND_HINT)
}

fn response_prefixes(depth: usize, content: &str) -> (&str, &str) {
    if contains_expand_hint_text(content) {
        ("  ", "  ")
    } else if depth > 2 {
        ("     ", "     ")
    } else {
        ("  ⎿  ", "     ")
    }
}

fn is_rendered_diff_line(line: &str) -> bool {
    let stripped = crate::utils::strip_ansi(line);
    if !stripped.starts_with("    ") {
        return false;
    }
    let after = &stripped[4..];
    if after.is_empty() {
        return false;
    }
    match after.as_bytes()[0] {
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

fn render_tool_result_content_lines(content: &str, width: usize, depth: usize) -> Vec<String> {
    let logical_lines: Vec<&str> = if content.is_empty() {
        vec![""]
    } else {
        content.split('\n').collect()
    };

    let (first_prefix, continuation_prefix) = response_prefixes(depth, content);
    let mut out = Vec::new();
    let mut first_non_diff = true;

    for logical_line in logical_lines {
        if is_rendered_diff_line(logical_line) {
            // Preserve diff lines exactly so ANSI backgrounds survive unchanged,
            // and avoid adding the normal tool-result prefix in front of them.
            out.push(logical_line.to_string());
            continue;
        }

        let initial_prefix = if first_non_diff {
            first_prefix
        } else {
            continuation_prefix
        };
        out.extend(wrap_visual_line(
            logical_line,
            width,
            initial_prefix,
            continuation_prefix,
        ));
        first_non_diff = false;
    }

    if out.is_empty() {
        out.push(first_prefix.to_string());
    }

    out
}

fn wrap_with_prefix(
    text: &str,
    width: usize,
    first_prefix: &str,
    continuation_prefix: &str,
) -> Vec<String> {
    let logical_lines: Vec<&str> = if text.is_empty() {
        vec![""]
    } else {
        text.split('\n').collect()
    };

    let mut out = Vec::new();
    let mut first_line = true;
    for logical_line in logical_lines {
        let initial_prefix = if first_line {
            first_prefix
        } else {
            continuation_prefix
        };
        let wrapped = wrap_visual_line(logical_line, width, initial_prefix, continuation_prefix);
        out.extend(wrapped);
        first_line = false;
    }

    if out.is_empty() {
        out.push(first_prefix.to_string());
    }

    out
}

pub(crate) fn wrap_visual_preview_lines(text: &str, width: usize) -> Vec<String> {
    let logical_lines: Vec<&str> = if text.is_empty() {
        vec![""]
    } else {
        text.split('\n').collect()
    };

    let mut out = Vec::new();
    for logical_line in logical_lines {
        out.extend(wrap_visual_line(logical_line, width, "", ""));
    }

    if out.is_empty() {
        out.push(String::new());
    }

    out
}

pub(super) fn wrap_visual_line(
    line: &str,
    width: usize,
    first_prefix: &str,
    continuation_prefix: &str,
) -> Vec<String> {
    if line.is_empty() {
        return vec![first_prefix.to_string()];
    }

    let bytes = line.as_bytes();
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut first = true;
    // Track SGR state across wrapped segments so styles don't bleed between
    // lines and so each continuation segment re-establishes the current color.
    let mut active_sgr: String = String::new();

    while start < line.len() {
        let prefix = if first {
            first_prefix
        } else {
            continuation_prefix
        };
        let prefix_width = visible_width(prefix);
        let available_width = width.saturating_sub(prefix_width).max(1);

        // Carry the active SGR state into this segment so resumed colors render.
        let carry_sgr = active_sgr.clone();
        let mut segment_has_ansi = !carry_sgr.is_empty();

        // Walk the line one char / ANSI-sequence at a time, respecting visible
        // width while keeping escape sequences intact.
        let mut i = start;
        let mut consumed_width = 0usize;
        let mut end = start;
        // Last byte offset where it's safe to break (after whitespace). Stored
        // as (segment_end, next_start, sgr_state_at_that_point).
        let mut last_break: Option<(usize, usize, String)> = None;

        while i < line.len() {
            if bytes[i] == 0x1b
                && let Some(len) = ansi_sequence_len(bytes, i)
            {
                let seq = &line[i..i + len];
                if is_sgr_sequence(seq) {
                    segment_has_ansi = true;
                    if is_reset_sgr(seq) {
                        active_sgr.clear();
                    } else {
                        active_sgr.push_str(seq);
                    }
                }
                i += len;
                end = i;
                continue;
            }

            let Some(ch) = line[i..].chars().next() else {
                break;
            };
            let ch_len = ch.len_utf8();
            let ch_width = char_width(ch);
            if consumed_width + ch_width > available_width {
                break;
            }
            consumed_width += ch_width;
            i += ch_len;
            end = i;
            if ch.is_whitespace() {
                last_break = Some((end, end, active_sgr.clone()));
            }
        }

        // Nothing fit: force a single-char advance so we never loop forever.
        if end == start {
            // Advance past any pending ANSI sequences first.
            if bytes[start] == 0x1b
                && let Some(len) = ansi_sequence_len(bytes, start)
            {
                let seq = &line[start..start + len];
                if is_sgr_sequence(seq) {
                    if is_reset_sgr(seq) {
                        active_sgr.clear();
                    } else {
                        active_sgr.push_str(seq);
                    }
                }
                out.push(format!("{prefix}{seq}"));
                start += len;
                first = false;
                continue;
            }
            let Some(ch) = line[start..].chars().next() else {
                break;
            };
            let next = start + ch.len_utf8();
            if carry_sgr.is_empty() {
                out.push(format!("{prefix}{}", &line[start..next]));
            } else {
                out.push(format!(
                    "{prefix}{carry_sgr}{}{}",
                    &line[start..next],
                    RESET_SGR
                ));
            }
            start = next;
            first = false;
            continue;
        }

        let (segment_end, next_start, sgr_after) = if end == line.len() {
            (line.len(), line.len(), active_sgr.clone())
        } else if let Some((seg_end, nxt, sgr)) = last_break.clone() {
            (seg_end, skip_leading_whitespace(line, nxt), sgr)
        } else {
            (end, end, active_sgr.clone())
        };

        let segment_text = &line[start..segment_end];
        if segment_has_ansi {
            out.push(format!(
                "{prefix}{carry_sgr}{}{}",
                trim_ansi_aware_end(segment_text),
                RESET_SGR
            ));
        } else {
            // Pure-ASCII segment: keep the legacy plain-text form so downstream
            // styling code (which sometimes pattern-matches on exact prefixes)
            // sees unchanged content.
            out.push(format!("{prefix}{}", segment_text.trim_end()));
        }

        // If we rewound to last_break, reflect the SGR state at that break.
        active_sgr = sgr_after;
        start = next_start;
        first = false;
    }

    if out.is_empty() {
        out.push(first_prefix.to_string());
    }

    out
}

fn is_sgr_sequence(seq: &str) -> bool {
    seq.starts_with("\x1b[") && seq.ends_with('m')
}

fn is_reset_sgr(seq: &str) -> bool {
    seq == "\x1b[0m" || seq == "\x1b[m"
}

/// ANSI-aware right-trim: strip trailing ASCII whitespace while keeping any
/// trailing escape sequences (e.g. the `\x1b[0m` reset) intact.
fn trim_ansi_aware_end(s: &str) -> &str {
    let bytes = s.as_bytes();
    let mut end = bytes.len();
    loop {
        // If the tail is an ANSI escape sequence, stop — keep it.
        if end >= 3 {
            // Walk backwards to find the start of a trailing ESC sequence.
            let mut probe = end;
            while probe > 0 && bytes[probe - 1] != 0x1b {
                probe -= 1;
            }
            if probe > 0
                && probe - 1 < end
                && bytes[probe - 1] == 0x1b
                && ansi_sequence_len(bytes, probe - 1) == Some(end - (probe - 1))
            {
                return &s[..end];
            }
        }
        if end == 0 {
            return &s[..end];
        }
        let last = bytes[end - 1];
        if last == b' ' || last == b'\t' {
            end -= 1;
            continue;
        }
        return &s[..end];
    }
}

fn skip_leading_whitespace(line: &str, start: usize) -> usize {
    let mut idx = start;
    while idx < line.len() {
        let Some(ch) = line[idx..].chars().next() else {
            break;
        };
        if !ch.is_whitespace() {
            break;
        }
        idx += ch.len_utf8();
    }
    idx
}
