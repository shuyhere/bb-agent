use super::projection::TranscriptProjection;
use super::transcript::BlockId;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewportAnchor {
    pub block_id: BlockId,
    pub screen_offset: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ViewportState {
    pub viewport_top: usize,
    pub viewport_height: usize,
    pub focused_row: Option<usize>,
    pub selected_block: Option<BlockId>,
    pub total_projected_rows: usize,
    pub auto_follow: bool,
}

impl ViewportState {
    pub fn new(viewport_height: usize) -> Self {
        Self {
            viewport_height,
            auto_follow: true,
            ..Self::default()
        }
    }

    pub fn with_total_rows(viewport_height: usize, total_projected_rows: usize) -> Self {
        let mut state = Self::new(viewport_height);
        state.set_total_rows(total_projected_rows);
        state.jump_to_bottom();
        state
    }

    pub fn visible_row_range(&self) -> std::ops::Range<usize> {
        let end = (self.viewport_top + self.viewport_height).min(self.total_projected_rows);
        self.viewport_top.min(end)..end
    }

    pub fn bottom_top(&self) -> usize {
        self.total_projected_rows
            .saturating_sub(self.viewport_height)
    }

    pub fn is_at_bottom(&self) -> bool {
        self.viewport_top >= self.bottom_top()
    }

    pub fn set_total_rows(&mut self, total_projected_rows: usize) {
        self.total_projected_rows = total_projected_rows;
        if self.auto_follow {
            self.viewport_top = self.bottom_top();
        } else {
            self.clamp_to_bounds();
        }
    }

    pub fn set_viewport_height(&mut self, viewport_height: usize) {
        self.viewport_height = viewport_height;
        self.clamp_to_bounds();
        if self.auto_follow {
            self.viewport_top = self.bottom_top();
        }
    }

    pub fn scroll_up(&mut self, rows: usize) {
        self.viewport_top = self.viewport_top.saturating_sub(rows);
        self.auto_follow = false;
    }

    pub fn scroll_down(&mut self, rows: usize) {
        self.viewport_top = (self.viewport_top + rows).min(self.bottom_top());
        if self.is_at_bottom() {
            self.auto_follow = true;
        }
    }

    pub fn jump_to_bottom(&mut self) {
        self.viewport_top = self.bottom_top();
        self.auto_follow = true;
    }

    pub fn jump_to_top(&mut self) {
        self.viewport_top = 0;
        self.auto_follow = false;
    }

    pub fn on_projection_changed(&mut self, projection: &TranscriptProjection) {
        self.total_projected_rows = projection.total_rows;
        if self.auto_follow {
            self.viewport_top = self.bottom_top();
        } else {
            self.clamp_to_bounds();
        }
    }

    pub fn capture_header_anchor(
        &self,
        projection: &TranscriptProjection,
        block_id: BlockId,
    ) -> Option<ViewportAnchor> {
        let row = projection
            .header_row_for_block(block_id)
            .or_else(|| projection.rows_for_block(block_id).map(|span| span.all_rows.start))?;
        Some(ViewportAnchor {
            block_id,
            screen_offset: row.saturating_sub(self.viewport_top),
        })
    }

    pub fn capture_anchor_for_row(
        &self,
        projection: &TranscriptProjection,
        row: usize,
    ) -> Option<ViewportAnchor> {
        let block_id = projection.row(row)?.block_id;
        self.capture_header_anchor(projection, block_id)
    }

    pub fn capture_top_anchor(&self, projection: &TranscriptProjection) -> Option<ViewportAnchor> {
        self.capture_anchor_for_row(projection, self.viewport_top)
    }

    pub fn preserve_anchor(
        &mut self,
        next_projection: &TranscriptProjection,
        anchor: &ViewportAnchor,
    ) {
        self.total_projected_rows = next_projection.total_rows;
        if let Some(next_row) = next_projection
            .header_row_for_block(anchor.block_id)
            .or_else(|| next_projection.rows_for_block(anchor.block_id).map(|span| span.all_rows.start))
        {
            self.viewport_top = next_row.saturating_sub(anchor.screen_offset);
            self.clamp_to_bounds();
        } else if self.auto_follow {
            self.viewport_top = self.bottom_top();
        } else {
            self.clamp_to_bounds();
        }
    }

    fn clamp_to_bounds(&mut self) {
        self.viewport_top = self.viewport_top.min(self.bottom_top());
        if let Some(row) = self.focused_row {
            self.focused_row = Some(row.min(self.total_projected_rows.saturating_sub(1)));
        }
    }
}
