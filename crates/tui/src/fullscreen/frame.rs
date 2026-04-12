mod chrome;
mod input;
mod transcript;

#[cfg(test)]
mod tests;

use super::{renderer::FrameBuffer, runtime::FullscreenState};
use chrome::{render_footer, render_header, render_status_lines};
#[cfg(test)]
pub(crate) use chrome::render_status;
pub(crate) use input::{
    attachment_chip_label, attachment_line_count, measure_approval_input, measure_input,
    visible_input_text,
};
use input::{
    blank_line, render_approval_dialog, render_approval_input, render_auth_dialog, render_input,
};
use transcript::render_transcript;

pub(crate) fn build_frame(state: &FullscreenState) -> FrameBuffer {
    let input_inner_width = state.size.width.max(1) as usize;
    let (visible_input, visible_cursor) =
        visible_input_text(&state.input, state.cursor, &state.cwd);
    let input_wrap = measure_input(&visible_input, visible_cursor, input_inner_width);
    let layout = state.current_layout();

    let mut lines = vec![blank_line(state.size.width as usize); state.size.height as usize];

    render_header(state, layout.header.width as usize)
        .into_iter()
        .enumerate()
        .for_each(|(offset, line)| {
            if let Some(slot) = lines.get_mut(layout.header.y as usize + offset) {
                *slot = line;
            }
        });

    render_transcript(
        state,
        &state.projection,
        layout.transcript.width as usize,
        layout.transcript.height as usize,
    )
    .into_iter()
    .enumerate()
    .for_each(|(offset, line)| {
        if let Some(slot) = lines.get_mut(layout.transcript.y as usize + offset) {
            *slot = line;
        }
    });

    if layout.status.height > 0 {
        render_status_lines(state, layout.status.width as usize, layout.status.height as usize)
            .into_iter()
            .enumerate()
            .for_each(|(offset, line)| {
                if let Some(slot) = lines.get_mut(layout.status.y as usize + offset) {
                    *slot = line;
                }
            });
    }

    let (input_lines, mut cursor) = if state.auth_dialog.is_some() {
        (
            vec![blank_line(layout.input.width as usize); layout.input.height as usize],
            None,
        )
    } else if state.approval_dialog.is_some() {
        render_approval_input(
            state,
            layout.input.y,
            layout.input.width as usize,
            layout.input.height as usize,
        )
    } else {
        render_input(
            state,
            layout.input.y,
            layout.input.width as usize,
            layout.input.height as usize,
            input_wrap,
        )
    };
    render_footer(
        state,
        layout.footer.width as usize,
        layout.footer.height as usize,
    )
    .into_iter()
    .enumerate()
    .for_each(|(offset, line)| {
        if let Some(slot) = lines.get_mut(layout.footer.y as usize + offset) {
            *slot = line;
        }
    });
    input_lines
        .into_iter()
        .enumerate()
        .for_each(|(offset, line)| {
            if let Some(slot) = lines.get_mut(layout.input.y as usize + offset) {
                *slot = line;
            }
        });

    if let Some((dialog_lines, dialog_cursor)) =
        render_auth_dialog(state, state.size.width as usize, state.size.height as usize)
    {
        for (y, line) in dialog_lines {
            if let Some(slot) = lines.get_mut(y) {
                *slot = line;
            }
        }
        cursor = dialog_cursor.or(cursor);
    }

    if let Some(dialog_lines) =
        render_approval_dialog(state, state.size.width as usize, state.size.height as usize)
    {
        for (y, line) in dialog_lines {
            if let Some(slot) = lines.get_mut(y) {
                *slot = line;
            }
        }
    }

    FrameBuffer { lines, cursor }
}
