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
    pub transcript: Rect,
    pub status: Rect,
    pub input: Rect,
}

pub fn compute_layout(size: Size, requested_input_lines: usize) -> FullscreenLayout {
    if size.height == 0 || size.width == 0 {
        return FullscreenLayout::default();
    }

    let status_height = 1u16.min(size.height);
    let available_for_input = size.height.saturating_sub(status_height);
    let min_input_height = available_for_input.min(3);
    let max_input_height = available_for_input.min(10).max(min_input_height);
    let requested_input_height = requested_input_lines as u16 + 2;
    let input_height = if max_input_height == 0 {
        0
    } else {
        requested_input_height.clamp(min_input_height.max(1), max_input_height)
    };
    let transcript_height = size.height.saturating_sub(status_height + input_height);

    let transcript = Rect {
        x: 0,
        y: 0,
        width: size.width,
        height: transcript_height,
    };
    let status = Rect {
        x: 0,
        y: transcript_height,
        width: size.width,
        height: status_height,
    };
    let input = Rect {
        x: 0,
        y: transcript_height + status_height,
        width: size.width,
        height: input_height,
    };

    FullscreenLayout {
        transcript,
        status,
        input,
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

        assert_eq!(layout.input.y + layout.input.height, 30);
        assert_eq!(layout.status.y + layout.status.height, layout.input.y);
        assert_eq!(
            layout.transcript.height + layout.status.height + layout.input.height,
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

        assert_eq!(layout.input.height, 10);
        assert_eq!(layout.transcript.height, 1);
    }
}
