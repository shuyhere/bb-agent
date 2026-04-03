use super::runtime::FullscreenState;
use super::transcript::BlockId;

impl FullscreenState {
    pub(crate) fn sync_focus_tracking(&mut self) {
        self.viewport.selected_block = self.focused_block;
        self.viewport.focused_row = self
            .focused_block
            .and_then(|block_id| self.focus_row_for_block(block_id));
    }

    pub(super) fn focus_first_visible_block(&mut self) {
        let visible = self.visible_header_blocks();
        if let Some(block_id) = visible.first().copied() {
            self.set_focused_block(Some(block_id));
        }
    }

    pub(super) fn focus_last_visible_block(&mut self) {
        let visible = self.visible_header_blocks();
        if let Some(block_id) = visible.last().copied() {
            self.set_focused_block(Some(block_id));
        }
    }

    pub(super) fn default_focus_block(&self) -> Option<BlockId> {
        self.visible_header_blocks()
            .last()
            .copied()
            .or_else(|| self.last_focusable_block())
            .or_else(|| self.first_focusable_block())
    }

    pub(crate) fn visible_header_blocks(&self) -> Vec<BlockId> {
        let mut visible = Vec::new();
        for row_index in self.viewport.visible_row_range() {
            let Some(row) = self.projection.row(row_index) else {
                continue;
            };
            if visible.last().copied() == Some(row.block_id) {
                continue;
            }
            if !visible.contains(&row.block_id) {
                visible.push(row.block_id);
            }
        }
        visible
    }

    pub(super) fn focusable_blocks(&self) -> Vec<BlockId> {
        self.projection
            .rows
            .iter()
            .filter(|row| self.focus_row_for_block(row.block_id) == Some(row.index))
            .map(|row| row.block_id)
            .collect()
    }

    pub(super) fn first_focusable_block(&self) -> Option<BlockId> {
        self.focusable_blocks().into_iter().next()
    }

    pub(super) fn last_focusable_block(&self) -> Option<BlockId> {
        self.focusable_blocks().into_iter().last()
    }

    pub(super) fn set_focused_block(&mut self, block_id: Option<BlockId>) {
        self.focused_block = block_id;
        self.sync_focus_tracking();
        self.dirty = true;
    }

    pub(super) fn focus_block(&mut self, block_id: BlockId) {
        self.focused_block = Some(block_id);
        self.viewport.auto_follow = false;
        self.sync_focus_tracking();
        self.ensure_focus_visible();
        self.dirty = true;
    }

    pub(super) fn focus_first(&mut self) {
        if let Some(block_id) = self.first_focusable_block() {
            self.focus_block(block_id);
        }
    }

    pub(super) fn focus_last(&mut self) {
        if let Some(block_id) = self.last_focusable_block() {
            self.focus_block(block_id);
        }
    }

    pub(super) fn move_focus(&mut self, step: isize) {
        let blocks = self.focusable_blocks();
        if blocks.is_empty() {
            return;
        }

        let current_index = self
            .focused_block
            .and_then(|block_id| blocks.iter().position(|candidate| *candidate == block_id))
            .unwrap_or_else(|| {
                if step.is_negative() {
                    blocks.len() - 1
                } else {
                    0
                }
            });

        let next_index = if step.is_negative() {
            current_index.saturating_sub(step.unsigned_abs())
        } else {
            (current_index + step as usize).min(blocks.len().saturating_sub(1))
        };

        self.focus_block(blocks[next_index]);
    }

    pub(super) fn page_move(&mut self, direction: isize) {
        let step = self.visible_header_blocks().len().max(1);
        for _ in 0..step {
            self.move_focus(direction.signum());
        }
    }

    pub(super) fn ensure_focus_visible(&mut self) {
        let Some(block_id) = self.focused_block else {
            self.sync_focus_tracking();
            return;
        };
        let Some(focus_row) = self.focus_row_for_block(block_id) else {
            self.sync_focus_tracking();
            return;
        };

        if self.viewport.viewport_height == 0 {
            self.sync_focus_tracking();
            return;
        }

        if focus_row < self.viewport.viewport_top {
            self.viewport.viewport_top = focus_row;
            self.viewport.auto_follow = false;
        } else if focus_row >= self.viewport.viewport_top + self.viewport.viewport_height {
            self.viewport.viewport_top = focus_row
                .saturating_add(1)
                .saturating_sub(self.viewport.viewport_height);
            self.viewport.auto_follow = false;
        }
        self.sync_focus_tracking();
    }

    pub(super) fn focus_row_for_block(&self, block_id: BlockId) -> Option<usize> {
        self.projection
            .header_row_for_block(block_id)
            .or_else(|| self.projection.rows_for_block(block_id).map(|span| span.all_rows.start))
    }
}
