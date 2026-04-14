use std::collections::{BTreeSet, HashMap};
use std::ops::Range;

mod compose;
mod render;

use compose::{collect_visible_blocks, compose_projection};
#[cfg(test)]
pub(super) use render::wrap_visual_line;
pub(crate) use render::wrap_visual_preview_lines;
use render::{render_collapsed_content_lines, render_content_lines, render_header_lines};

use super::transcript::{BlockId, Transcript};

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
    width: usize,
    rows: Vec<ProjectedRow>,
    total_rows: usize,
    block_rows: HashMap<BlockId, BlockRowSpan>,
}

impl TranscriptProjection {
    pub fn width(&self) -> usize {
        self.width
    }

    pub fn rows(&self) -> &[ProjectedRow] {
        &self.rows
    }

    pub fn total_rows(&self) -> usize {
        self.total_rows
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

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

    pub(super) fn push_row(&mut self, row: ProjectedRow) {
        self.rows.push(row);
    }

    pub(super) fn insert_block_rows(&mut self, block_id: BlockId, span: BlockRowSpan) {
        self.block_rows.insert(block_id, span);
    }

    pub(super) fn row_count(&self) -> usize {
        self.rows.len()
    }

    pub(super) fn set_total_rows(&mut self) {
        self.total_rows = self.rows.len();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct OrderedBlock {
    block_id: BlockId,
    depth: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct CachedBlockRows {
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
            || self.projection.is_empty())
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
                        render_collapsed_content_lines(block, width, entry.depth)
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

#[cfg(test)]
mod tests;
