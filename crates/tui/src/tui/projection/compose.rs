use std::collections::HashMap;

use crate::tui::transcript::{BlockId, Transcript};

use super::{
    BlockRowSpan, CachedBlockRows, OrderedBlock, ProjectedRow, ProjectedRowKind,
    TranscriptProjection,
};

pub(super) fn collect_visible_blocks(transcript: &Transcript) -> Vec<OrderedBlock> {
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

pub(super) fn compose_projection(
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

        let all_start = projection.row_count();
        let header_start = projection.row_count();
        for line in &rows.header_lines {
            let index = projection.row_count();
            projection.push_row(ProjectedRow {
                index,
                block_id: entry.block_id,
                kind: ProjectedRowKind::Header,
                depth: entry.depth,
                text: line.clone(),
            });
        }
        let header_end = projection.row_count();

        let content_start = projection.row_count();
        for line in &rows.content_lines {
            let index = projection.row_count();
            projection.push_row(ProjectedRow {
                index,
                block_id: entry.block_id,
                kind: ProjectedRowKind::Content,
                depth: entry.depth,
                text: line.clone(),
            });
        }
        let content_end = projection.row_count();
        let all_end = projection.row_count();

        projection.insert_block_rows(
            entry.block_id,
            BlockRowSpan {
                all_rows: all_start..all_end,
                header_rows: header_start..header_end,
                content_rows: content_start..content_end,
            },
        );

        // Spacer rows are inserted only when returning to the same or a shallower depth.
        // That keeps nested tool groups visually compact while preserving a stable blank-line
        // rhythm between sibling/root blocks.
        if should_insert_spacer(entry, block_order.get(idx + 1)) {
            let index = projection.row_count();
            projection.push_row(ProjectedRow {
                index,
                block_id: entry.block_id,
                kind: ProjectedRowKind::Spacer,
                depth: entry.depth,
                text: String::new(),
            });
        }
        idx += 1;
    }

    projection.set_total_rows();
    projection
}

fn should_insert_spacer(current: &OrderedBlock, next: Option<&OrderedBlock>) -> bool {
    next.is_some_and(|next| next.depth <= current.depth)
}
