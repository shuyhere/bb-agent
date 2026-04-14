use super::*;
use crate::tui::transcript::{BlockKind, NewBlock, Transcript};

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
    assert_eq!(projection.total_rows(), 1);
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
        &updated.rows()[initial_first.all_rows.clone()],
        &initial.rows()[initial_first.all_rows]
    );
    assert_ne!(
        &updated.rows()[initial_second.all_rows.clone()],
        &initial.rows()[initial_second.all_rows]
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

    assert!(narrow.total_rows() > wide.total_rows());
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
    let user_rows = &projection.rows()[user_span.content_rows.clone()];
    assert!(user_span.header_rows.is_empty());
    assert_eq!(user_rows.len(), 1);
    assert!(user_rows[0].text.contains("hello"));
    assert_eq!(
        projection.rows()[user_span.all_rows.end].kind,
        ProjectedRowKind::Spacer
    );

    let tool_span = projection.rows_for_block(tool).expect("tool span");
    assert_eq!(tool_span.header_rows.len(), 1);
    let tool_header = &projection.rows()[tool_span.header_rows.start];
    assert!(tool_header.text.contains("bash"));
    assert!(!tool_header.text.contains("tool bash"));
    let tool_rows = &projection.rows()[tool_span.content_rows.clone()];
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
    let result_rows = &projection.rows()[result_span.content_rows.clone()];
    assert_eq!(result_rows.len(), 1);
    assert!(result_rows[0].text.contains("done"));
}

#[test]
fn collapsed_compaction_shows_compact_context_preview_and_expand_hint() {
    let mut transcript = Transcript::new();
    let compaction = transcript.append_root_block(
        NewBlock::new(BlockKind::SystemNote, "compaction")
            .with_content(
                "[compaction: 12345 tokens summarized]\n\n## Goal\nFinish the cleanup\n\n## Constraints\nKeep it native\n\n## Next Steps\n1. Build\n2. Verify"
            )
            .with_expandable(true)
            .with_collapsed(true),
    );

    let mut projector = TranscriptProjector::new();
    let projection = projector.project(&mut transcript, 80, &std::collections::HashSet::new());
    let span = projection
        .rows_for_block(compaction)
        .expect("compaction span");

    let header_rows = &projection.rows()[span.header_rows.clone()];
    assert!(
        header_rows
            .iter()
            .any(|row| row.text.contains("[Compact Context]"))
    );

    let content_rows = &projection.rows()[span.content_rows.clone()];
    let content_text = content_rows
        .iter()
        .map(|row| row.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");

    assert!(content_text.contains("## Goal"));
    assert!(content_text.contains("Finish the cleanup"));
    assert!(content_text.contains("## Constraints"));
    assert!(content_text.contains("Keep it native"));
    assert!(!content_text.contains("## Next Steps"));
    assert!(!content_text.contains("1. Build"));
    assert!(content_text.contains(crate::ui_hints::TOOL_EXPAND_HINT));
    assert!(content_text.contains("Keep it native\n\nCtrl+Shift+O to expand"));
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
        projection.rows()[result_span.all_rows.end].kind,
        ProjectedRowKind::Spacer
    );
    assert_eq!(
        projection.rows()[result_span.all_rows.end + 1].block_id,
        note
    );
    assert_eq!(projection.rows()[note_span.all_rows.start].block_id, note);
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
    let rows = &projection.rows()[span.content_rows.clone()];

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
    let rows = &projection.rows()[span.content_rows.clone()];

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
    let rows = &projection.rows()[span.content_rows.clone()];

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
            NewBlock::new(BlockKind::ToolResult, "output").with_content(format!(
                "Read 1 file ({})",
                crate::ui_hints::TOOL_EXPAND_HINT
            )),
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
            NewBlock::new(BlockKind::ToolResult, "output").with_content(format!(
                "Read 1 file ({})",
                crate::ui_hints::TOOL_EXPAND_HINT
            )),
        )
        .expect("result2");

    let mut projector = TranscriptProjector::new();
    let projection = projector.project(&mut transcript, 80, &std::collections::HashSet::new());
    let tool_span1 = projection.rows_for_block(read1).expect("read1 span");
    let tool_span2 = projection.rows_for_block(read2).expect("read2 span");
    let result_span1 = projection.rows_for_block(result1).expect("result1 span");
    let result_span2 = projection.rows_for_block(result2).expect("result2 span");
    let header1 = &projection.rows()[tool_span1.header_rows.start];
    let header2 = &projection.rows()[tool_span2.header_rows.start];
    let rows1 = &projection.rows()[result_span1.content_rows.clone()];
    let rows2 = &projection.rows()[result_span2.content_rows.clone()];

    assert!(header1.text.contains("Read(/tmp/a.txt)"));
    assert!(header2.text.contains("Read(/tmp/b.txt)"));
    assert!(rows1.iter().any(|row| {
        row.text.contains(&format!(
            "Read 1 file ({})",
            crate::ui_hints::TOOL_EXPAND_HINT
        ))
    }));
    assert!(rows2.iter().any(|row| {
        row.text.contains(&format!(
            "Read 1 file ({})",
            crate::ui_hints::TOOL_EXPAND_HINT
        ))
    }));
}

#[test]
fn wrap_visual_line_preserves_ansi_escape_sequences() {
    // Long ANSI-colored tokens simulating a syntax-highlighted source line.
    // Without ANSI awareness, the wrapper counted every byte of
    // `\x1b[38;2;192;197;206m` as a visible column and split the escape
    // sequence across wrapped lines, leaking `192;197;206m` and `0m` into
    // the rendered output as literal text.
    let line = format!(
        "{red}use{reset} {blue}bb_core::agent_session{reset}::{green}ModelRef{reset};",
        red = "\x1b[38;2;192;197;206m",
        blue = "\x1b[38;2;102;153;204m",
        green = "\x1b[38;2;181;189;104m",
        reset = "\x1b[0m",
    );

    let wrapped = super::wrap_visual_line(&line, 20, "", "  ");
    assert!(!wrapped.is_empty(), "should produce at least one segment");

    for segment in &wrapped {
        // No visible leak of SGR parameter bytes — bare `192;197;206m` etc.
        // should never appear without a leading ESC.
        let stripped = crate::utils::strip_ansi(segment);
        assert!(
            !stripped.contains("192;197;206m"),
            "stripped segment still shows raw SGR params: {stripped:?}",
        );
        assert!(
            !stripped.contains("38;2;"),
            "stripped segment still shows truecolor params: {stripped:?}",
        );
        assert!(
            !stripped.ends_with('m') || !stripped.contains(';'),
            "segment tail looks like an SGR parameter list: {stripped:?}",
        );
    }

    // Concatenating the stripped-ANSI segments should reconstruct the
    // visible text (order-preserving, modulo trailing whitespace trimming).
    let joined_visible: String = wrapped
        .iter()
        .map(|s| crate::utils::strip_ansi(s))
        .collect::<Vec<_>>()
        .join("");
    assert!(joined_visible.contains("use"));
    assert!(joined_visible.contains("bb_core"));
    assert!(joined_visible.contains("ModelRef"));

    // Every segment must respect the 20-column width budget.
    for segment in &wrapped {
        let vw = crate::utils::visible_width(segment);
        assert!(
            vw <= 20,
            "segment exceeds visible width budget ({vw} > 20): {segment:?}",
        );
    }
}
