#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Size {
    pub width: u16,
    pub height: u16,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FullscreenLayout {
    pub header: Rect,
    pub transcript: Rect,
    pub status: Rect,
    pub input: Rect,
    pub footer: Rect,
}

pub fn compute_layout(size: Size, requested_input_lines: usize) -> FullscreenLayout {
    compute_layout_with_footer(size, requested_input_lines, if size.height >= 14 { 2 } else { 0 })
}

pub fn compute_layout_with_footer(
    size: Size,
    requested_input_lines: usize,
    requested_footer_lines: u16,
) -> FullscreenLayout {
    if size.height == 0 || size.width == 0 {
        return FullscreenLayout::default();
    }

    let header_height = if size.height >= 8 { 3 } else { 0 };
    let max_footer_height = size.height.saturating_sub(header_height + 1);
    let footer_height = requested_footer_lines.min(max_footer_height);
    let status_height = 1u16.min(size.height.saturating_sub(header_height + footer_height));
    let available_for_input = size.height.saturating_sub(header_height + status_height + footer_height);
    let min_input_height = available_for_input.min(3);
    let max_input_height = available_for_input.min(10).max(min_input_height);
    let requested_input_height = requested_input_lines as u16 + 2;
    let input_height = if max_input_height == 0 {
        0
    } else {
        requested_input_height.clamp(min_input_height.max(1), max_input_height)
    };
    let transcript_height = size
        .height
        .saturating_sub(header_height + status_height + input_height + footer_height);

    let header = Rect {
        x: 0,
        y: 0,
        width: size.width,
        height: header_height,
    };
    let transcript = Rect {
        x: 0,
        y: header_height,
        width: size.width,
        height: transcript_height,
    };
    let status = Rect {
        x: 0,
        y: header_height + transcript_height,
        width: size.width,
        height: status_height,
    };
    let input = Rect {
        x: 0,
        y: header_height + transcript_height + status_height,
        width: size.width,
        height: input_height,
    };
    let footer = Rect {
        x: 0,
        y: header_height + transcript_height + status_height + input_height,
        width: size.width,
        height: footer_height,
    };

    FullscreenLayout {
        header,
        transcript,
        status,
        input,
        footer,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_input_fixed_at_bottom() {
        let layout = compute_layout(
            Size {
                width: 100,
                height: 30,
            },
            4,
        );

        assert_eq!(layout.header.height, 3);
        assert_eq!(layout.footer.height, 2);
        assert_eq!(layout.footer.y + layout.footer.height, 30);
        assert_eq!(layout.input.y + layout.input.height, layout.footer.y);
        assert_eq!(layout.status.y + layout.status.height, layout.input.y);
        assert_eq!(
            layout.header.height
                + layout.transcript.height
                + layout.status.height
                + layout.input.height
                + layout.footer.height,
            30
        );
    }

    #[test]
    fn clamps_input_growth() {
        let layout = compute_layout(
            Size {
                width: 80,
                height: 12,
            },
            50,
        );

        assert_eq!(layout.header.height, 3);
        assert_eq!(layout.footer.height, 0);
        assert_eq!(layout.input.height, 8);
        assert_eq!(layout.transcript.height, 0);
    }

    #[test]
    fn omits_header_on_tiny_terminals() {
        let layout = compute_layout(
            Size {
                width: 80,
                height: 6,
            },
            1,
        );

        assert_eq!(layout.header.height, 0);
        assert_eq!(layout.footer.height, 0);
        assert_eq!(layout.input.y + layout.input.height, 6);
        assert_eq!(layout.status.y + layout.status.height, layout.input.y);
    }

    #[test]
    fn reserves_extra_footer_space_for_menus() {
        let layout = compute_layout_with_footer(
            Size {
                width: 80,
                height: 20,
            },
            1,
            8,
        );

        assert_eq!(layout.footer.height, 8);
        assert_eq!(layout.input.y + layout.input.height, layout.footer.y);
    }
}
