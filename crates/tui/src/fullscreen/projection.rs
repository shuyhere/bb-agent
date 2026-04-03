use std::collections::HashMap;
use std::ops::Range;

use crate::markdown::MarkdownRenderer;
use crate::utils::{char_width, visible_width};

use super::transcript::{BlockId, BlockKind, Transcript, TranscriptBlock};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectedRowKind {
    Header,
    Content,
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
    block_rows: HashMap<BlockId, CachedBlockRows>,
    projection: TranscriptProjection,
}

impl TranscriptProjector {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn project(&mut self, transcript: &mut Transcript, width: usize) -> TranscriptProjection {
        let width = width.max(1);
        let width_changed = self.width != width;
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
            || order_changed
            || !dirty_blocks.is_empty()
            || self.projection.rows.is_empty())
        {
            return self.projection.clone();
        }

        self.width = width;
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

    for entry in block_order {
        let Some(rows) = cached_rows.get(&entry.block_id) else {
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
    }

    projection.total_rows = projection.rows.len();
    projection
}

fn render_header_lines(block: &TranscriptBlock, width: usize, depth: usize) -> Vec<String> {
    let indent = "  ".repeat(depth);
    let expandable = block.expandable || !block.children.is_empty();
    let marker = if expandable {
        if block.collapsed { "▸" } else { "▾" }
    } else {
        "•"
    };
    let first_prefix = format!("{indent}{marker} ");
    let continuation_prefix = format!("{}  ", indent);
    let header_text = if block.title.trim().is_empty() {
        kind_label(&block.kind).to_string()
    } else {
        format!("{} {}", kind_label(&block.kind), block.title)
    };

    wrap_with_prefix(&header_text, width, &first_prefix, &continuation_prefix)
}

fn render_content_lines(block: &TranscriptBlock, width: usize, depth: usize) -> Vec<String> {
    let prefix = format!("{}  ", "  ".repeat(depth + 1));
    let mut lines = if block.content.trim().is_empty() {
        Vec::new()
    } else {
        match block.kind {
            BlockKind::UserMessage | BlockKind::AssistantMessage | BlockKind::Thinking => {
                render_markdown_content_lines(&block.content, width, &prefix)
            }
            _ => wrap_with_prefix(&block.content, width, &prefix, &prefix),
        }
    };
    apply_visual_padding(block, &mut lines);
    lines
}

fn render_markdown_content_lines(text: &str, width: usize, prefix: &str) -> Vec<String> {
    let available_width = width.saturating_sub(visible_width(prefix)).max(1);
    let mut renderer = MarkdownRenderer::new(text);
    renderer
        .render(available_width as u16)
        .into_iter()
        .map(|line| {
            if line.is_empty() {
                prefix.to_string()
            } else {
                format!("{prefix}{line}")
            }
        })
        .collect()
}

fn apply_visual_padding(block: &TranscriptBlock, lines: &mut Vec<String>) {
    match block.kind {
        BlockKind::UserMessage => {
            if !lines.is_empty() {
                lines.insert(0, String::new());
                lines.push(String::new());
            }
        }
        BlockKind::AssistantMessage | BlockKind::Thinking => {
            if !lines.is_empty() {
                lines.insert(0, String::new());
            }
        }
        BlockKind::ToolUse | BlockKind::ToolResult => {
            if lines.is_empty() {
                lines.push(String::new());
            } else {
                lines.insert(0, String::new());
                lines.push(String::new());
            }
        }
        BlockKind::SystemNote => {}
    }
}

fn kind_label(kind: &BlockKind) -> &'static str {
    match kind {
        BlockKind::UserMessage => "you",
        BlockKind::AssistantMessage => "bb",
        BlockKind::Thinking => "thinking",
        BlockKind::ToolUse => "tool",
        BlockKind::ToolResult => "result",
        BlockKind::SystemNote => "note",
    }
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
    for logical_line in logical_lines {
        let wrapped = wrap_visual_line(logical_line, width, first_prefix, continuation_prefix);
        out.extend(wrapped);
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
mod tests {
    use super::*;
    use crate::fullscreen::transcript::{BlockKind, NewBlock, Transcript};

    #[test]
    fn projects_collapsed_children_out_of_view() {
        let mut transcript = Transcript::new();
        let root = transcript.append_root_block(
            NewBlock::new(BlockKind::AssistantMessage, "assistant").with_content("hello"),
        );
        transcript
            .append_child_block(root, NewBlock::new(BlockKind::Thinking, "thought"))
            .expect("child block should be appended");
        transcript
            .set_collapsed(root, true)
            .expect("collapse should succeed");

        let mut projector = TranscriptProjector::new();
        let projection = projector.project(&mut transcript, 40);
        assert_eq!(projection.total_rows, 1);
    }

    #[test]
    fn reuses_cached_rows_for_clean_blocks() {
        let mut transcript = Transcript::new();
        let first = transcript
            .append_root_block(NewBlock::new(BlockKind::SystemNote, "first").with_content("alpha"));
        let second = transcript
            .append_root_block(NewBlock::new(BlockKind::SystemNote, "second").with_content("beta"));

        let mut projector = TranscriptProjector::new();
        let initial = projector.project(&mut transcript, 40);
        let initial_first = initial
            .rows_for_block(first)
            .cloned()
            .expect("first span should exist");
        let initial_second = initial
            .rows_for_block(second)
            .cloned()
            .expect("second span should exist");

        transcript
            .append_streamed_content(second, " gamma")
            .expect("streaming append should succeed");
        let updated = projector.project(&mut transcript, 40);

        assert_eq!(
            &updated.rows[initial_first.all_rows.clone()],
            &initial.rows[initial_first.all_rows]
        );
        assert_ne!(
            &updated.rows[initial_second.all_rows.clone()],
            &initial.rows[initial_second.all_rows]
        );
    }

    #[test]
    fn width_change_rewraps_existing_cached_blocks() {
        let mut transcript = Transcript::new();
        transcript.append_root_block(
            NewBlock::new(BlockKind::AssistantMessage, "assistant")
                .with_content("this line wraps differently when the width changes"),
        );

        let mut projector = TranscriptProjector::new();
        let wide = projector.project(&mut transcript, 40);
        let narrow = projector.project(&mut transcript, 18);

        assert!(narrow.total_rows > wide.total_rows);
    }

    #[test]
    fn clean_projection_call_leaves_transcript_clean() {
        let mut transcript = Transcript::new();
        transcript.append_root_block(
            NewBlock::new(BlockKind::AssistantMessage, "assistant").with_content("hello"),
        );

        let mut projector = TranscriptProjector::new();
        let _ = projector.project(&mut transcript, 40);
        assert!(!transcript.has_dirty_blocks());

        let _ = projector.project(&mut transcript, 40);
        assert!(!transcript.has_dirty_blocks());
    }

    #[test]
    fn projection_adds_visual_padding_for_chat_like_blocks() {
        let mut transcript = Transcript::new();
        let user = transcript.append_root_block(
            NewBlock::new(BlockKind::UserMessage, "prompt").with_content("hello"),
        );
        let tool = transcript.append_root_block(
            NewBlock::new(BlockKind::ToolUse, "bash").with_content("timeout 5s"),
        );

        let mut projector = TranscriptProjector::new();
        let projection = projector.project(&mut transcript, 80);

        let user_span = projection.rows_for_block(user).expect("user span");
        let user_rows = &projection.rows[user_span.content_rows.clone()];
        assert!(user_rows.first().expect("user top pad").text.is_empty());
        assert!(user_rows.last().expect("user bottom pad").text.is_empty());

        let tool_span = projection.rows_for_block(tool).expect("tool span");
        let tool_rows = &projection.rows[tool_span.content_rows.clone()];
        assert!(tool_rows.first().expect("tool top pad").text.is_empty());
        assert!(tool_rows.last().expect("tool bottom pad").text.is_empty());
    }

    #[test]
    fn assistant_blocks_use_markdown_renderer() {
        let mut transcript = Transcript::new();
        let assistant = transcript.append_root_block(
            NewBlock::new(BlockKind::AssistantMessage, "assistant")
                .with_content("# Heading\n\n- item"),
        );

        let mut projector = TranscriptProjector::new();
        let projection = projector.project(&mut transcript, 80);
        let span = projection.rows_for_block(assistant).expect("assistant span");
        let rows = &projection.rows[span.content_rows.clone()];

        assert!(rows.iter().any(|row| row.text.contains("\x1b[")));
        assert!(rows.iter().any(|row| row.text.contains("Heading") || row.text.contains("item")));
    }
}
