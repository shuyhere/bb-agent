use std::collections::{BTreeSet, HashMap};
use std::ops::Range;

use crate::markdown::MarkdownRenderer;
use crate::utils::{char_width, visible_width};

use super::transcript::{BlockId, BlockKind, Transcript, TranscriptBlock};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectedRowKind {
    Header,
    Content,
    Spacer,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectedRow {
    pub index: usize,
    pub block_id: BlockId,
    pub kind: ProjectedRowKind,
    pub depth: usize,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockRowSpan {
    pub all_rows: Range<usize>,
    pub header_rows: Range<usize>,
    pub content_rows: Range<usize>,
}

#[derive(Clone, Debug, Default)]
pub struct TranscriptProjection {
    pub width: usize,
    pub rows: Vec<ProjectedRow>,
    pub total_rows: usize,
    pub block_rows: HashMap<BlockId, BlockRowSpan>,
}

impl TranscriptProjection {
    pub fn row(&self, row: usize) -> Option<&ProjectedRow> {
        self.rows.get(row)
    }

    pub fn rows_for_block(&self, block_id: BlockId) -> Option<&BlockRowSpan> {
        self.block_rows.get(&block_id)
    }

    pub fn header_row_for_block(&self, block_id: BlockId) -> Option<usize> {
        self.block_rows
            .get(&block_id)
            .map(|span| span.header_rows.start)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct OrderedBlock {
    block_id: BlockId,
    depth: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct CachedBlockRows {
    depth: usize,
    header_lines: Vec<String>,
    content_lines: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct TranscriptProjector {
    width: usize,
    block_order: Vec<OrderedBlock>,
    expanded_tool_blocks: BTreeSet<BlockId>,
    block_rows: HashMap<BlockId, CachedBlockRows>,
    projection: TranscriptProjection,
}

impl TranscriptProjector {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn project(
        &mut self,
        transcript: &mut Transcript,
        width: usize,
        expanded_tool_blocks: &std::collections::HashSet<BlockId>,
    ) -> TranscriptProjection {
        let width = width.max(1);
        let width_changed = self.width != width;
        let next_expanded_tool_blocks = expanded_tool_blocks
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        let expanded_changed = self.expanded_tool_blocks != next_expanded_tool_blocks;
        let mut dirty_blocks = transcript.take_dirty_blocks();
        if width_changed {
            dirty_blocks.extend(transcript.all_block_ids());
        }
        let next_order = collect_visible_blocks(transcript);
        let order_changed = self.block_order != next_order;

        if width_changed {
            self.block_rows.clear();
        }

        if !(width_changed
            || expanded_changed
            || order_changed
            || !dirty_blocks.is_empty()
            || self.projection.rows.is_empty())
        {
            return self.projection.clone();
        }

        self.width = width;
        self.expanded_tool_blocks = next_expanded_tool_blocks;
        self.block_rows
            .retain(|block_id, _| next_order.iter().any(|entry| entry.block_id == *block_id));

        for entry in &next_order {
            let should_render = width_changed
                || dirty_blocks.contains(&entry.block_id)
                || self
                    .block_rows
                    .get(&entry.block_id)
                    .map(|cached| cached.depth != entry.depth)
                    .unwrap_or(true);
            if !should_render {
                continue;
            }

            let Some(block) = transcript.block(entry.block_id) else {
                continue;
            };
            self.block_rows.insert(
                entry.block_id,
                CachedBlockRows {
                    depth: entry.depth,
                    header_lines: render_header_lines(block, width, entry.depth),
                    content_lines: if block.collapsed {
                        Vec::new()
                    } else {
                        render_content_lines(block, width, entry.depth)
                    },
                },
            );
        }

        self.block_order = next_order;
        self.projection = compose_projection(width, &self.block_order, &self.block_rows);
        self.projection.clone()
    }
}

fn collect_visible_blocks(transcript: &Transcript) -> Vec<OrderedBlock> {
    let mut blocks = Vec::new();
    for root_id in transcript.root_blocks() {
        collect_visible_block_recursive(transcript, *root_id, 0, &mut blocks);
    }
    blocks
}

fn collect_visible_block_recursive(
    transcript: &Transcript,
    block_id: BlockId,
    depth: usize,
    out: &mut Vec<OrderedBlock>,
) {
    let Some(block) = transcript.block(block_id) else {
        return;
    };

    out.push(OrderedBlock { block_id, depth });
    if block.collapsed {
        return;
    }

    for child_id in &block.children {
        collect_visible_block_recursive(transcript, *child_id, depth + 1, out);
    }
}

fn compose_projection(
    width: usize,
    block_order: &[OrderedBlock],
    cached_rows: &HashMap<BlockId, CachedBlockRows>,
) -> TranscriptProjection {
    let mut projection = TranscriptProjection {
        width,
        ..TranscriptProjection::default()
    };

    let mut idx = 0usize;
    while idx < block_order.len() {
        let entry = &block_order[idx];

        let Some(rows) = cached_rows.get(&entry.block_id) else {
            idx += 1;
            continue;
        };

        let all_start = projection.rows.len();
        let header_start = projection.rows.len();
        for line in &rows.header_lines {
            let index = projection.rows.len();
            projection.rows.push(ProjectedRow {
                index,
                block_id: entry.block_id,
                kind: ProjectedRowKind::Header,
                depth: entry.depth,
                text: line.clone(),
            });
        }
        let header_end = projection.rows.len();

        let content_start = projection.rows.len();
        for line in &rows.content_lines {
            let index = projection.rows.len();
            projection.rows.push(ProjectedRow {
                index,
                block_id: entry.block_id,
                kind: ProjectedRowKind::Content,
                depth: entry.depth,
                text: line.clone(),
            });
        }
        let content_end = projection.rows.len();
        let all_end = projection.rows.len();

        projection.block_rows.insert(
            entry.block_id,
            BlockRowSpan {
                all_rows: all_start..all_end,
                header_rows: header_start..header_end,
                content_rows: content_start..content_end,
            },
        );

        if should_insert_spacer(entry, block_order.get(idx + 1)) {
            let index = projection.rows.len();
            projection.rows.push(ProjectedRow {
                index,
                block_id: entry.block_id,
                kind: ProjectedRowKind::Spacer,
                depth: entry.depth,
                text: String::new(),
            });
        }
        idx += 1;
    }

    projection.total_rows = projection.rows.len();
    projection
}

fn is_summary_note(block: &TranscriptBlock) -> bool {
    block.kind == BlockKind::SystemNote
        && matches!(block.title.as_str(), "branch summary" | "compaction")
}

fn render_header_lines(block: &TranscriptBlock, width: usize, _depth: usize) -> Vec<String> {
    if block.kind == BlockKind::ToolUse {
        let header_text = if let Some((base, _status)) = block.title.rsplit_once(" • ") {
            base.trim().to_string()
        } else if block.title.trim().is_empty() {
            "Tool".to_string()
        } else {
            block.title.trim().to_string()
        };
        return wrap_with_prefix(&header_text, width, "● ", "  ");
    }

    if is_summary_note(block) {
        let header_text = match block.title.as_str() {
            "branch summary" => "◆ Branch Summary",
            "compaction" => "◆ Compaction Summary",
            other => other,
        };
        return wrap_with_prefix(header_text, width, "", "");
    }

    Vec::new()
}

fn should_insert_spacer(current: &OrderedBlock, next: Option<&OrderedBlock>) -> bool {
    next.is_some_and(|next| next.depth <= current.depth)
}

fn render_content_lines(block: &TranscriptBlock, width: usize, depth: usize) -> Vec<String> {
    if block.content.trim().is_empty() {
        return if is_summary_note(block) {
            vec!["╰─ ".to_string()]
        } else {
            Vec::new()
        };
    }

    match block.kind {
        BlockKind::UserMessage => wrap_with_prefix(&block.content, width, "❯ ", "  "),
        BlockKind::AssistantMessage => render_markdown_content_lines(&block.content, width, "", ""),
        BlockKind::Thinking => render_markdown_content_lines(&block.content, width, "", ""),
        BlockKind::SystemNote => {
            if is_summary_note(block) {
                render_summary_block_content(&block.content, width)
            } else {
                wrap_with_prefix(&block.content, width, "", "")
            }
        }
        BlockKind::ToolUse => {
            let (first_prefix, continuation_prefix) = response_prefixes(depth, &block.content);
            wrap_with_prefix(&block.content, width, first_prefix, continuation_prefix)
        }
        BlockKind::ToolResult => render_tool_result_content_lines(&block.content, width, depth),
    }
}

fn render_summary_block_content(text: &str, width: usize) -> Vec<String> {
    let mut lines = wrap_with_prefix(text, width, "│  ", "│  ");
    lines.push("╰─ ".to_string());
    lines
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

fn response_prefixes(depth: usize, content: &str) -> (&str, &str) {
    if content.contains("(click or use Ctrl+Shift+O to enter tool expand mode)") {
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

fn wrap_visual_line(
    line: &str,
    width: usize,
    first_prefix: &str,
    continuation_prefix: &str,
) -> Vec<String> {
    if line.is_empty() {
        return vec![first_prefix.to_string()];
    }

    let mut out = Vec::new();
    let mut start = 0usize;
    let mut first = true;

    while start < line.len() {
        let prefix = if first {
            first_prefix
        } else {
            continuation_prefix
        };
        let prefix_width = visible_width(prefix);
        let available_width = width.saturating_sub(prefix_width).max(1);

        let mut end = start;
        let mut consumed_width = 0usize;
        let mut last_break: Option<usize> = None;

        for (rel_idx, ch) in line[start..].char_indices() {
            let abs_idx = start + rel_idx;
            let ch_width = char_width(ch);
            if consumed_width + ch_width > available_width {
                break;
            }
            consumed_width += ch_width;
            end = abs_idx + ch.len_utf8();
            if ch.is_whitespace() {
                last_break = Some(end);
            }
        }

        if end == start {
            let ch = line[start..]
                .chars()
                .next()
                .expect("line should have a char");
            let next = start + ch.len_utf8();
            out.push(format!("{prefix}{}", &line[start..next]));
            start = next;
            first = false;
            continue;
        }

        let (segment_end, next_start) = if end == line.len() {
            (line.len(), line.len())
        } else if let Some(break_end) = last_break {
            (break_end, skip_leading_whitespace(line, break_end))
        } else {
            (end, end)
        };

        out.push(format!("{prefix}{}", line[start..segment_end].trim_end()));
        start = next_start;
        first = false;
    }

    if out.is_empty() {
        out.push(first_prefix.to_string());
    }

    out
}

fn skip_leading_whitespace(line: &str, start: usize) -> usize {
    let mut idx = start;
    while idx < line.len() {
        let ch = line[idx..].chars().next().expect("line should have a char");
        if !ch.is_whitespace() {
            break;
        }
        idx += ch.len_utf8();
    }
    idx
}

#[cfg(test)]
mod tests;
