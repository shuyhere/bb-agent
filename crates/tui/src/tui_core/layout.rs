use super::{OverlayAnchor, OverlayOptions, ResolvedLayout, SizeValue, TUI};

impl TUI {
    /// Resolve overlay layout from options and terminal dimensions.
    pub(crate) fn resolve_overlay_layout(
        options: &OverlayOptions,
        overlay_height: usize,
        term_width: usize,
        term_height: usize,
    ) -> ResolvedLayout {
        let margin = &options.margin;
        let margin_top = margin.top;
        let margin_right = margin.right;
        let margin_bottom = margin.bottom;
        let margin_left = margin.left;

        let avail_w = term_width.saturating_sub(margin_left + margin_right).max(1);
        let avail_h = term_height
            .saturating_sub(margin_top + margin_bottom)
            .max(1);

        let mut width = options
            .width
            .as_ref()
            .map(|size| size.resolve(term_width))
            .unwrap_or_else(|| avail_w.min(80));
        if let Some(min_width) = options.min_width {
            width = width.max(min_width);
        }
        width = width.clamp(1, avail_w);

        let max_height = options.max_height.as_ref().map(|size| {
            let height = size.resolve(term_height);
            height.clamp(1, avail_h)
        });

        let eff_height = max_height
            .map(|max_height| overlay_height.min(max_height))
            .unwrap_or(overlay_height);

        let row = if let Some(size) = options.row.as_ref() {
            match size {
                SizeValue::Percent(percent) => {
                    let max_row = avail_h.saturating_sub(eff_height);
                    margin_top + ((max_row as f32 * percent / 100.0).floor() as usize)
                }
                SizeValue::Absolute(value) => *value,
            }
        } else {
            Self::resolve_anchor_row(options.anchor, eff_height, avail_h, margin_top)
        };

        let col = if let Some(size) = options.col.as_ref() {
            match size {
                SizeValue::Percent(percent) => {
                    let max_col = avail_w.saturating_sub(width);
                    margin_left + ((max_col as f32 * percent / 100.0).floor() as usize)
                }
                SizeValue::Absolute(value) => *value,
            }
        } else {
            Self::resolve_anchor_col(options.anchor, width, avail_w, margin_left)
        };

        let row = (row as i32 + options.offset_y).max(0) as usize;
        let col = (col as i32 + options.offset_x).max(0) as usize;

        let row = row.clamp(
            margin_top,
            term_height.saturating_sub(margin_bottom + eff_height),
        );
        let col = col.clamp(margin_left, term_width.saturating_sub(margin_right + width));

        ResolvedLayout {
            width,
            row,
            col,
            max_height,
        }
    }

    fn resolve_anchor_row(
        anchor: OverlayAnchor,
        height: usize,
        avail_h: usize,
        margin_top: usize,
    ) -> usize {
        match anchor {
            OverlayAnchor::TopLeft | OverlayAnchor::TopCenter | OverlayAnchor::TopRight => {
                margin_top
            }
            OverlayAnchor::BottomLeft
            | OverlayAnchor::BottomCenter
            | OverlayAnchor::BottomRight => margin_top + avail_h.saturating_sub(height),
            _ => margin_top + avail_h.saturating_sub(height) / 2,
        }
    }

    fn resolve_anchor_col(
        anchor: OverlayAnchor,
        width: usize,
        avail_w: usize,
        margin_left: usize,
    ) -> usize {
        match anchor {
            OverlayAnchor::TopLeft | OverlayAnchor::LeftCenter | OverlayAnchor::BottomLeft => {
                margin_left
            }
            OverlayAnchor::TopRight | OverlayAnchor::RightCenter | OverlayAnchor::BottomRight => {
                margin_left + avail_w.saturating_sub(width)
            }
            _ => margin_left + avail_w.saturating_sub(width) / 2,
        }
    }
}
