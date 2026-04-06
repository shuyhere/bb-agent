use crate::component::{BOTTOM_ANCHOR_MARKER, Component};
use crate::terminal::Terminal;
use crate::utils::{extract_segments, visible_width};

use super::{OverlayAnchor, SEGMENT_RESET, TUI};

impl TUI {
    /// Render the component tree to the terminal, compositing visible overlays on top.
    pub fn render(&mut self) {
        if self.stopped {
            return;
        }
        self.render_requested = false;
        let width = self.terminal.columns();
        let height = self.terminal.rows();
        let mut lines = self.root.render(width);
        lines = Self::apply_bottom_anchor(lines, height as usize);

        if !self.overlay_stack.is_empty() {
            lines = self.composite_overlays(lines, width as usize, height as usize);
        }

        self.renderer.render(&lines, &mut self.terminal);
    }

    pub(super) fn apply_bottom_anchor(lines: Vec<String>, term_height: usize) -> Vec<String> {
        let mut cleaned = Vec::with_capacity(lines.len());
        let mut anchor_idx: Option<usize> = None;

        for line in lines {
            if line.contains(BOTTOM_ANCHOR_MARKER) {
                anchor_idx = Some(cleaned.len());
                let stripped = line.replace(BOTTOM_ANCHOR_MARKER, "");
                if !stripped.is_empty() {
                    cleaned.push(stripped);
                }
            } else {
                cleaned.push(line);
            }
        }

        if let Some(anchor_idx) = anchor_idx
            && cleaned.len() < term_height
        {
            let pad = term_height - cleaned.len();
            cleaned.splice(
                anchor_idx..anchor_idx,
                std::iter::repeat_n(String::new(), pad),
            );
        }

        cleaned
    }

    /// Composite all visible overlays into the base content lines.
    fn composite_overlays(
        &self,
        lines: Vec<String>,
        term_width: usize,
        term_height: usize,
    ) -> Vec<String> {
        let mut result = lines;

        let visible: Vec<usize> = self
            .overlay_stack
            .iter()
            .enumerate()
            .filter(|(_, entry)| !entry.hidden)
            .map(|(index, _)| index)
            .collect();

        if visible.is_empty() {
            return result;
        }

        struct Rendered {
            lines: Vec<String>,
            row: usize,
            col: usize,
            width: usize,
        }

        let mut rendered: Vec<Rendered> = Vec::new();
        let mut min_lines_needed = result.len();

        for &idx in &visible {
            let entry = &self.overlay_stack[idx];

            if entry.options.anchor == OverlayAnchor::Bottom
                && entry.options.col.is_none()
                && entry.options.row.is_none()
                && entry.options.width.is_none()
            {
                let overlay_lines = entry.component.render(term_width as u16);
                let overlay_len = overlay_lines.len();
                if overlay_len == 0 {
                    continue;
                }
                while result.len() < overlay_len {
                    result.push(String::new());
                }
                let start = result.len() - overlay_len;
                for (offset, line) in overlay_lines.into_iter().enumerate() {
                    result[start + offset] = line;
                }
                continue;
            }

            let layout0 = Self::resolve_overlay_layout(&entry.options, 0, term_width, term_height);
            let render_width = layout0.width.max(1) as u16;

            let mut overlay_lines = entry.component.render(render_width);
            if let Some(max_height) = layout0.max_height {
                overlay_lines.truncate(max_height);
            }
            if overlay_lines.is_empty() {
                continue;
            }

            let layout = Self::resolve_overlay_layout(
                &entry.options,
                overlay_lines.len(),
                term_width,
                term_height,
            );

            min_lines_needed = min_lines_needed.max(layout.row + overlay_lines.len());
            rendered.push(Rendered {
                lines: overlay_lines,
                row: layout.row,
                col: layout.col,
                width: layout.width,
            });
        }

        let working_height = min_lines_needed.max(result.len());
        while result.len() < working_height {
            result.push(String::new());
        }

        let viewport_start = working_height.saturating_sub(term_height);

        for rendered_overlay in &rendered {
            for (offset, overlay_line) in rendered_overlay.lines.iter().enumerate() {
                let idx = viewport_start + rendered_overlay.row + offset;
                if idx < result.len() {
                    let overlay = if visible_width(overlay_line) > rendered_overlay.width {
                        crate::utils::truncate_to_width(overlay_line, rendered_overlay.width)
                    } else {
                        overlay_line.clone()
                    };
                    result[idx] = Self::composite_line_at(
                        &result[idx],
                        &overlay,
                        rendered_overlay.col,
                        rendered_overlay.width,
                        term_width,
                    );
                }
            }
        }

        result
    }

    /// Splice overlay content into a base line at a specific column.
    pub(crate) fn composite_line_at(
        base_line: &str,
        overlay_line: &str,
        start_col: usize,
        overlay_width: usize,
        total_width: usize,
    ) -> String {
        let after_start = start_col + overlay_width;
        let after_width = total_width.saturating_sub(after_start);

        let seg = extract_segments(base_line, start_col, after_start, after_width);

        let before_pad = start_col.saturating_sub(seg.before_width);
        let overlay_vw = visible_width(overlay_line);
        let overlay_pad = overlay_width.saturating_sub(overlay_vw);

        let actual_before = start_col.max(seg.before_width);
        let actual_overlay = overlay_width.max(overlay_vw);
        let after_target = total_width.saturating_sub(actual_before + actual_overlay);
        let after_pad = after_target.saturating_sub(seg.after_width);

        let mut out = String::with_capacity(
            seg.before.len()
                + before_pad
                + SEGMENT_RESET.len()
                + overlay_line.len()
                + overlay_pad
                + SEGMENT_RESET.len()
                + seg.after.len()
                + after_pad,
        );
        out.push_str(&seg.before);
        for _ in 0..before_pad {
            out.push(' ');
        }
        out.push_str(SEGMENT_RESET);
        out.push_str(overlay_line);
        for _ in 0..overlay_pad {
            out.push(' ');
        }
        out.push_str(SEGMENT_RESET);
        out.push_str(&seg.after);
        for _ in 0..after_pad {
            out.push(' ');
        }

        let result_width = visible_width(&out);
        if result_width > total_width {
            crate::utils::truncate_to_width(&out, total_width)
        } else {
            out
        }
    }
}
