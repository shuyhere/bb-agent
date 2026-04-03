use std::collections::HashMap;
use std::ops::Range;

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

#[derive(Default)]
pub struct TranscriptProjector;

impl TranscriptProjector {
    pub fn new() -> Self {
        Self
    }

    pub fn project(&mut self, transcript: &Transcript, width: usize) -> TranscriptProjection {
        let width = width.max(1);
        let mut projection = TranscriptProjection {
            width,
            ..TranscriptProjection::default()
        };

        for root_id in transcript.root_blocks() {
            self.project_block(&mut projection, transcript, *root_id, 0);
        }

        projection.total_rows = projection.rows.len();
        projection
    }

    fn project_block(
        &mut self,
        projection: &mut TranscriptProjection,
        transcript: &Transcript,
        block_id: BlockId,
        depth: usize,
    ) {
        let Some(block) = transcript.block(block_id).cloned() else {
            return;
        };

        let all_start = projection.rows.len();
        let header_start = projection.rows.len();
        for line in render_header_lines(&block, projection.width, depth) {
            let index = projection.rows.len();
            projection.rows.push(ProjectedRow {
                index,
                block_id,
                kind: ProjectedRowKind::Header,
                depth,
                text: line,
            });
        }
        let header_end = projection.rows.len();

        let content_start = projection.rows.len();
        if !block.collapsed {
            for line in render_content_lines(&block, projection.width, depth) {
                let index = projection.rows.len();
                projection.rows.push(ProjectedRow {
                    index,
                    block_id,
                    kind: ProjectedRowKind::Content,
                    depth,
                    text: line,
                });
            }
        }
        let content_end = projection.rows.len();

        if !block.collapsed {
            for child_id in &block.children {
                self.project_block(projection, transcript, *child_id, depth + 1);
            }
        }

        let all_end = projection.rows.len();
        projection.block_rows.insert(
            block_id,
            BlockRowSpan {
                all_rows: all_start..all_end,
                header_rows: header_start..header_end,
                content_rows: content_start..content_end,
            },
        );
    }
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
    if block.content.trim().is_empty() {
        return Vec::new();
    }

    let prefix = format!("{}  ", "  ".repeat(depth + 1));
    wrap_with_prefix(&block.content, width, &prefix, &prefix)
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
            let ch = line[start..].chars().next().unwrap();
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
        let ch = line[idx..].chars().next().unwrap();
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
            .unwrap();
        transcript.set_collapsed(root, true).unwrap();

        let mut projector = TranscriptProjector::new();
        let projection = projector.project(&transcript, 40);
        assert_eq!(projection.total_rows, 1);
    }
}
