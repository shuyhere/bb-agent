use super::*;
use crate::fullscreen::transcript::{BlockKind, NewBlock, Transcript};

#[test]
fn projects_collapsed_children_out_of_view() {
    let mut transcript = Transcript::new();
    let root = transcript
        .append_root_block(NewBlock::new(BlockKind::ToolUse, "$ ls").with_content("timeout 5s"));
    transcript
        .append_child_block(
            root,
            NewBlock::new(BlockKind::ToolResult, "output").with_content("done"),
        )
        .expect("child block should be appended");
    transcript
        .set_collapsed(root, true)
        .expect("collapse should succeed");

    let mut projector = TranscriptProjector::new();
    let projection = projector.project(&mut transcript, 40, &std::collections::HashSet::new());
    assert_eq!(projection.total_rows, 1);
}

#[test]
fn reuses_cached_rows_for_clean_blocks() {
    let mut transcript = Transcript::new();
    let first = transcript
        .append_root_block(NewBlock::new(BlockKind::SystemNote, "first").with_content("alpha"));
    let second = transcript
        .append_root_block(NewBlock::new(BlockKind::SystemNote, "second").with_content("beta"));

    let mut projector = TranscriptProjector::new();
    let initial = projector.project(&mut transcript, 40, &std::collections::HashSet::new());
    let initial_first = initial
        .rows_for_block(first)
        .cloned()
        .expect("first span should exist");
    let initial_second = initial
        .rows_for_block(second)
        .cloned()
        .expect("second span should exist");

    transcript
        .append_streamed_content(second, " gamma")
        .expect("streaming append should succeed");
    let updated = projector.project(&mut transcript, 40, &std::collections::HashSet::new());

    assert_eq!(
        &updated.rows[initial_first.all_rows.clone()],
        &initial.rows[initial_first.all_rows]
    );
    assert_ne!(
        &updated.rows[initial_second.all_rows.clone()],
        &initial.rows[initial_second.all_rows]
    );
}

#[test]
fn width_change_rewraps_existing_cached_blocks() {
    let mut transcript = Transcript::new();
    transcript.append_root_block(
        NewBlock::new(BlockKind::AssistantMessage, "assistant")
            .with_content("this line wraps differently when the width changes"),
    );

    let mut projector = TranscriptProjector::new();
    let wide = projector.project(&mut transcript, 40, &std::collections::HashSet::new());
    let narrow = projector.project(&mut transcript, 18, &std::collections::HashSet::new());

    assert!(narrow.total_rows > wide.total_rows);
}

#[test]
fn clean_projection_call_leaves_transcript_clean() {
    let mut transcript = Transcript::new();
    transcript.append_root_block(
        NewBlock::new(BlockKind::AssistantMessage, "assistant").with_content("hello"),
    );

    let mut projector = TranscriptProjector::new();
    let _ = projector.project(&mut transcript, 40, &std::collections::HashSet::new());
    assert!(!transcript.has_dirty_blocks());

    let _ = projector.project(&mut transcript, 40, &std::collections::HashSet::new());
    assert!(!transcript.has_dirty_blocks());
}

#[test]
fn projection_uses_uniform_spacers_instead_of_inner_padding() {
    let mut transcript = Transcript::new();
    let user = transcript
        .append_root_block(NewBlock::new(BlockKind::UserMessage, "prompt").with_content("hello"));
    let tool = transcript
        .append_root_block(NewBlock::new(BlockKind::ToolUse, "bash").with_content("timeout 5s"));

    let mut projector = TranscriptProjector::new();
    let projection = projector.project(&mut transcript, 80, &std::collections::HashSet::new());

    let user_span = projection.rows_for_block(user).expect("user span");
    let user_rows = &projection.rows[user_span.content_rows.clone()];
    assert!(user_span.header_rows.is_empty());
    assert_eq!(user_rows.len(), 1);
    assert!(user_rows[0].text.contains("hello"));
    assert_eq!(
        projection.rows[user_span.all_rows.end].kind,
        ProjectedRowKind::Spacer
    );

    let tool_span = projection.rows_for_block(tool).expect("tool span");
    assert_eq!(tool_span.header_rows.len(), 1);
    let tool_header = &projection.rows[tool_span.header_rows.start];
    assert!(tool_header.text.contains("bash"));
    assert!(!tool_header.text.contains("tool bash"));
    let tool_rows = &projection.rows[tool_span.content_rows.clone()];
    assert!(
        !tool_rows
            .first()
            .expect("tool first body row")
            .text
            .is_empty()
    );

    let result = transcript
        .append_child_block(
            tool,
            NewBlock::new(BlockKind::ToolResult, "output").with_content("done"),
        )
        .expect("tool result");
    let projection = projector.project(&mut transcript, 80, &std::collections::HashSet::new());
    let result_span = projection.rows_for_block(result).expect("result span");
    assert!(result_span.header_rows.is_empty());
    let result_rows = &projection.rows[result_span.content_rows.clone()];
    assert_eq!(result_rows.len(), 1);
    assert!(result_rows[0].text.contains("done"));
}

#[test]
fn projection_inserts_spacer_after_nested_group_when_returning_to_parent_level() {
    let mut transcript = Transcript::new();
    let assistant = transcript.append_root_block(
        NewBlock::new(BlockKind::AssistantMessage, "assistant").with_content("hello"),
    );
    let tool = transcript
        .append_child_block(
            assistant,
            NewBlock::new(BlockKind::ToolUse, "bash").with_content("echo hi"),
        )
        .expect("tool");
    let result = transcript
        .append_child_block(
            tool,
            NewBlock::new(BlockKind::ToolResult, "output").with_content("done"),
        )
        .expect("result");
    let note = transcript
        .append_root_block(NewBlock::new(BlockKind::SystemNote, "status").with_content("next"));

    let mut projector = TranscriptProjector::new();
    let projection = projector.project(&mut transcript, 80, &std::collections::HashSet::new());
    let result_span = projection.rows_for_block(result).expect("result span");
    let note_span = projection.rows_for_block(note).expect("note span");

    assert_eq!(
        projection.rows[result_span.all_rows.end].kind,
        ProjectedRowKind::Spacer
    );
    assert_eq!(projection.rows[result_span.all_rows.end + 1].block_id, note);
    assert_eq!(projection.rows[note_span.all_rows.start].block_id, note);
}

#[test]
fn assistant_blocks_use_markdown_renderer() {
    let mut transcript = Transcript::new();
    let assistant = transcript.append_root_block(
        NewBlock::new(BlockKind::AssistantMessage, "assistant").with_content("# Heading\n\n- item"),
    );

    let mut projector = TranscriptProjector::new();
    let projection = projector.project(&mut transcript, 80, &std::collections::HashSet::new());
    let span = projection
        .rows_for_block(assistant)
        .expect("assistant span");
    let rows = &projection.rows[span.content_rows.clone()];

    assert!(rows.iter().any(|row| row.text.contains("\x1b[")));
    assert!(
        rows.iter()
            .any(|row| row.text.contains("Heading") || row.text.contains("item"))
    );
}

#[test]
fn user_slash_commands_remain_visible_in_content_rows() {
    let mut transcript = Transcript::new();
    let user = transcript
        .append_root_block(NewBlock::new(BlockKind::UserMessage, "prompt").with_content("/help"));

    let mut projector = TranscriptProjector::new();
    let projection = projector.project(&mut transcript, 80, &std::collections::HashSet::new());
    let span = projection.rows_for_block(user).expect("user span");
    let rows = &projection.rows[span.content_rows.clone()];

    assert!(rows.iter().any(|row| row.text.contains("/help")));
}

#[test]
fn assistant_table_rows_remain_separate_in_projection() {
    let mut transcript = Transcript::new();
    let assistant = transcript.append_root_block(
        NewBlock::new(BlockKind::AssistantMessage, "assistant").with_content(
            "| Crate | Purpose |\n| --- | --- |\n| cli | CLI entry point |\n| core | Agent loop |",
        ),
    );

    let mut projector = TranscriptProjector::new();
    let projection = projector.project(&mut transcript, 100, &std::collections::HashSet::new());
    let span = projection
        .rows_for_block(assistant)
        .expect("assistant span");
    let rows = &projection.rows[span.content_rows.clone()];

    assert!(rows.iter().any(|row| {
        let plain = crate::utils::strip_ansi(&row.text);
        plain.contains("Crate") && plain.contains("Purpose")
    }));
    assert!(rows.iter().any(|row| {
        let plain = crate::utils::strip_ansi(&row.text);
        plain.contains("cli") && plain.contains("CLI entry point")
    }));
    assert!(rows.iter().any(|row| {
        let plain = crate::utils::strip_ansi(&row.text);
        plain.contains("core") && plain.contains("Agent loop")
    }));
}

#[test]
fn projection_keeps_consecutive_same_type_tools_visible() {
    let mut transcript = Transcript::new();
    let assistant = transcript.append_root_block(
        NewBlock::new(BlockKind::AssistantMessage, "assistant").with_content("done"),
    );
    let read1 = transcript
        .append_child_block(
            assistant,
            NewBlock::new(BlockKind::ToolUse, "Read(/tmp/a.txt) • done").with_content(""),
        )
        .expect("read1");
    let result1 = transcript
        .append_child_block(
            read1,
            NewBlock::new(BlockKind::ToolResult, "output")
                .with_content("Read 1 file (Ctrl+Shift+O tool expand)"),
        )
        .expect("result1");
    let read2 = transcript
        .append_child_block(
            assistant,
            NewBlock::new(BlockKind::ToolUse, "Read(/tmp/b.txt) • done").with_content(""),
        )
        .expect("read2");
    let result2 = transcript
        .append_child_block(
            read2,
            NewBlock::new(BlockKind::ToolResult, "output")
                .with_content("Read 1 file (Ctrl+Shift+O tool expand)"),
        )
        .expect("result2");

    let mut projector = TranscriptProjector::new();
    let projection = projector.project(&mut transcript, 80, &std::collections::HashSet::new());
    let tool_span1 = projection.rows_for_block(read1).expect("read1 span");
    let tool_span2 = projection.rows_for_block(read2).expect("read2 span");
    let result_span1 = projection.rows_for_block(result1).expect("result1 span");
    let result_span2 = projection.rows_for_block(result2).expect("result2 span");
    let header1 = &projection.rows[tool_span1.header_rows.start];
    let header2 = &projection.rows[tool_span2.header_rows.start];
    let rows1 = &projection.rows[result_span1.content_rows.clone()];
    let rows2 = &projection.rows[result_span2.content_rows.clone()];

    assert!(header1.text.contains("Read(/tmp/a.txt)"));
    assert!(header2.text.contains("Read(/tmp/b.txt)"));
    assert!(
        rows1
            .iter()
            .any(|row| row.text.contains("Read 1 file (Ctrl+Shift+O tool expand)"))
    );
    assert!(
        rows2
            .iter()
            .any(|row| row.text.contains("Read 1 file (Ctrl+Shift+O tool expand)"))
    );
}
