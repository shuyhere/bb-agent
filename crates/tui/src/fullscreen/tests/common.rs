use super::*;

pub(super) fn sample_state() -> (FullscreenState, BlockId, BlockId, BlockId) {
    let mut transcript = Transcript::new();
    let intro = transcript.append_root_block(
        NewBlock::new(BlockKind::SystemNote, "intro").with_content("foundation"),
    );
    let tool = transcript.append_root_block(
        NewBlock::new(BlockKind::ToolUse, "read config")
            .with_content("read /tmp/demo.txt")
            .with_expandable(true),
    );
    let result = transcript
        .append_child_block(
            tool,
            NewBlock::new(BlockKind::ToolResult, "output").with_content("hello world"),
        )
        .expect("tool result should be appended");

    let state = FullscreenState::new(
        FullscreenAppConfig {
            transcript,
            ..FullscreenAppConfig::default()
        },
        Size {
            width: 80,
            height: 16,
        },
    );
    (state, intro, tool, result)
}

pub(super) fn scrolling_state() -> (FullscreenState, Vec<BlockId>) {
    let mut transcript = Transcript::new();
    let mut blocks = Vec::new();
    for idx in 0..10 {
        let block_id = transcript.append_root_block(
            NewBlock::new(BlockKind::AssistantMessage, format!("message {idx}"))
                .with_content(format!("line {idx}\nmore detail {idx}")),
        );
        blocks.push(block_id);
    }

    let state = FullscreenState::new(
        FullscreenAppConfig {
            transcript,
            ..FullscreenAppConfig::default()
        },
        Size {
            width: 60,
            height: 10,
        },
    );
    (state, blocks)
}

pub(super) fn screen_row_for_header(state: &FullscreenState, block_id: BlockId) -> u16 {
    let header_row = state
        .projection
        .header_row_for_block(block_id)
        .expect("header row should exist");
    let local_row = header_row.saturating_sub(state.viewport.viewport_top);
    let layout = state.current_layout();
    layout.transcript.y + local_row as u16
}

pub(super) fn screen_row_for_first_content(state: &FullscreenState, block_id: BlockId) -> u16 {
    let content_row = state
        .projection
        .rows_for_block(block_id)
        .expect("content rows should exist")
        .content_rows
        .start;
    let local_row = content_row.saturating_sub(state.viewport.viewport_top);
    let layout = state.current_layout();
    layout.transcript.y + local_row as u16
}
