use super::super::*;

#[test]
fn streaming_updates_do_not_force_auto_follow_back_to_bottom() {
    let mut transcript = Transcript::new();
    for idx in 0..8 {
        transcript.append_root_block(
            NewBlock::new(BlockKind::SystemNote, format!("note {idx}"))
                .with_content(format!("line {idx}")),
        );
    }

    let mut state = TuiState::new(
        TuiAppConfig {
            transcript,
            ..TuiAppConfig::default()
        },
        Size {
            width: 80,
            height: 12,
        },
    );
    state.viewport.jump_to_top();
    state.projection_dirty = true;
    state.prepare_for_render();
    let top_before = state.viewport.viewport_top;

    let _ = state.apply_command(TuiCommand::TurnStart { turn_index: 0 });
    let _ = state.apply_command(TuiCommand::TextDelta("hello".to_string()));
    state.prepare_for_render();

    assert!(!state.viewport.auto_follow);
    assert_eq!(state.viewport.viewport_top, top_before);
}

#[test]
fn focused_transcript_anchor_is_preserved_during_streaming() {
    let mut transcript = Transcript::new();
    let first = transcript
        .append_root_block(NewBlock::new(BlockKind::SystemNote, "first").with_content("one"));
    transcript
        .append_root_block(NewBlock::new(BlockKind::SystemNote, "second").with_content("two"));
    transcript
        .append_root_block(NewBlock::new(BlockKind::SystemNote, "third").with_content("three"));

    let mut state = TuiState::new(
        TuiAppConfig {
            transcript,
            ..TuiAppConfig::default()
        },
        Size {
            width: 80,
            height: 12,
        },
    );
    state.mode = TuiMode::Transcript;
    state.focused_block = Some(first);
    state.viewport.jump_to_top();
    state.viewport.auto_follow = false;
    state.sync_focus_tracking();
    let anchor_before = state
        .viewport
        .capture_header_anchor(&state.projection, first)
        .expect("anchor should exist");

    let _ = state.apply_command(TuiCommand::TurnStart { turn_index: 0 });
    let _ = state.apply_command(TuiCommand::TextDelta("delta".into()));
    state.prepare_for_render();

    let anchor_after = state
        .viewport
        .capture_header_anchor(&state.projection, first)
        .expect("anchor should still exist");
    assert_eq!(anchor_after.screen_offset, anchor_before.screen_offset);
    assert_eq!(state.focused_block, Some(first));
}

#[test]
fn command_deltas_update_only_shared_transcript_blocks() {
    let mut state = TuiState::new(
        TuiAppConfig::default(),
        Size {
            width: 80,
            height: 24,
        },
    );

    let _ = state.apply_command(TuiCommand::TurnStart { turn_index: 0 });
    let _ = state.apply_command(TuiCommand::ThinkingDelta("thinking".into()));
    let _ = state.apply_command(TuiCommand::ToolCallStart {
        id: "tool-1".into(),
        name: "bash".into(),
    });
    let _ = state.apply_command(TuiCommand::ToolCallDelta {
        id: "tool-1".into(),
        args: "{\"command\":\"ls\"}".into(),
    });

    let assistant = state.transcript.root_blocks()[0];
    let assistant_block = state
        .transcript
        .block(assistant)
        .expect("assistant root should exist");
    let tool_use_before_result = state
        .transcript
        .block(assistant_block.children[1])
        .expect("tool use block should exist before result");
    assert!(tool_use_before_result.children.is_empty());

    let _ = state.apply_command(TuiCommand::ToolResult {
        id: "tool-1".into(),
        name: "bash".into(),
        content: vec![ContentBlock::Text {
            text: "file.txt".into(),
        }],
        details: None,
        artifact_path: None,
        is_error: false,
    });
    let _ = state.apply_command(TuiCommand::TextDelta("done".into()));
    state.prepare_for_render();

    let assistant = state.transcript.root_blocks()[0];
    let assistant_block = state
        .transcript
        .block(assistant)
        .expect("assistant root should exist");
    assert_eq!(assistant_block.kind, BlockKind::AssistantMessage);
    assert_eq!(assistant_block.children.len(), 3);

    let thinking = state
        .transcript
        .block(assistant_block.children[0])
        .expect("thinking block should exist");
    assert_eq!(thinking.kind, BlockKind::Thinking);
    assert_eq!(thinking.content, "thinking");

    let tool_use = state
        .transcript
        .block(assistant_block.children[1])
        .expect("tool use block should exist");
    assert_eq!(tool_use.kind, BlockKind::ToolUse);
    assert!(tool_use.title.contains("ls"));

    let tool_result = state
        .transcript
        .block(tool_use.children[0])
        .expect("tool result block should exist");
    assert_eq!(tool_result.kind, BlockKind::ToolResult);
    assert!(tool_result.content.contains("file.txt"));

    let response = state
        .transcript
        .block(assistant_block.children[2])
        .expect("assistant response block should exist");
    assert_eq!(response.kind, BlockKind::AssistantMessage);
    assert_eq!(response.content, "done");
}

#[test]
fn resumed_tool_transcript_can_expand_from_historical_tool_state() {
    let mut transcript = Transcript::new();
    let assistant = transcript.append_root_block(
        NewBlock::new(BlockKind::AssistantMessage, "assistant").with_content(""),
    );
    let tool_use_id = transcript
        .append_child_block(
            assistant,
            NewBlock::new(BlockKind::ToolUse, "Bash(printf lines)").with_expandable(true),
        )
        .expect("tool use");
    let collapsed = format_tool_result_content(
        "bash",
        &[ContentBlock::Text {
            text: (1..=14)
                .map(|i| format!("line {i}"))
                .collect::<Vec<_>>()
                .join("\n"),
        }],
        None,
        None,
        false,
        false,
    );
    let tool_result_id = transcript
        .append_child_block(
            tool_use_id,
            NewBlock::new(BlockKind::ToolResult, "output").with_content(collapsed),
        )
        .expect("tool result");

    let mut tool_states = std::collections::HashMap::new();
    tool_states.insert(
        "tool-1".to_string(),
        HistoricalToolState {
            name: "bash".to_string(),
            raw_args: serde_json::json!({ "command": "printf '...lines...'" }).to_string(),
            tool_use_id,
            tool_result_id: Some(tool_result_id),
            result_content: Some(vec![ContentBlock::Text {
                text: (1..=14)
                    .map(|i| format!("line {i}"))
                    .collect::<Vec<_>>()
                    .join("\n"),
            }]),
            result_details: None,
            artifact_path: None,
            is_error: false,
        },
    );

    let mut state = TuiState::new(
        TuiAppConfig::default(),
        Size {
            width: 80,
            height: 24,
        },
    );
    let _ = state.apply_command(TuiCommand::SetTranscriptWithToolStates {
        transcript,
        tool_states,
    });
    state.prepare_for_render();
    state.on_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL));
    state.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    let tool_use = state
        .transcript
        .block(tool_use_id)
        .expect("tool use after expand");
    let tool_result = state
        .transcript
        .block(tool_result_id)
        .expect("tool result after expand");
    assert!(state.expanded_tool_blocks.contains(&tool_use_id));
    assert!(tool_result.content.contains("line 14"));
    assert!(
        !tool_result
            .content
            .contains(crate::ui_hints::TOOL_EXPAND_HINT)
    );
    assert!(tool_use.title.contains("Bash"));
}

#[test]
fn historical_bash_result_does_not_render_missing_exit_code_as_negative_one() {
    let mut transcript = Transcript::new();
    let assistant = transcript.append_root_block(
        NewBlock::new(BlockKind::AssistantMessage, "assistant").with_content(""),
    );
    let tool_use_id = transcript
        .append_child_block(
            assistant,
            NewBlock::new(BlockKind::ToolUse, "Bash(printf ok)").with_expandable(true),
        )
        .expect("tool use");
    let tool_result_id = transcript
        .append_child_block(
            tool_use_id,
            NewBlock::new(BlockKind::ToolResult, "output").with_content("ok"),
        )
        .expect("tool result");

    let mut tool_states = std::collections::HashMap::new();
    tool_states.insert(
        "tool-1".to_string(),
        HistoricalToolState {
            name: "bash".to_string(),
            raw_args: serde_json::json!({ "command": "printf ok" }).to_string(),
            tool_use_id,
            tool_result_id: Some(tool_result_id),
            result_content: Some(vec![ContentBlock::Text { text: "ok".into() }]),
            result_details: Some(serde_json::json!({ "exitCode": null })),
            artifact_path: None,
            is_error: false,
        },
    );

    let mut state = TuiState::new(
        TuiAppConfig::default(),
        Size {
            width: 80,
            height: 24,
        },
    );
    let _ = state.apply_command(TuiCommand::SetTranscriptWithToolStates {
        transcript,
        tool_states,
    });
    state.prepare_for_render();

    let tool_result = state
        .transcript
        .block(tool_result_id)
        .expect("tool result after restore");
    assert!(!tool_result.content.contains("exit code: -1"));
    assert!(tool_result.content.contains("ok"));
}

#[test]
fn tool_result_appears_only_when_finished_and_enter_expands_output() {
    let mut state = TuiState::new(
        TuiAppConfig::default(),
        Size {
            width: 80,
            height: 24,
        },
    );

    let _ = state.apply_command(TuiCommand::TurnStart { turn_index: 0 });
    let _ = state.apply_command(TuiCommand::ToolCallStart {
        id: "tool-1".into(),
        name: "bash".into(),
    });
    let _ = state.apply_command(TuiCommand::ToolCallDelta {
        id: "tool-1".into(),
        args: serde_json::json!({ "command": "printf 'a\nb\nc\nd\ne\nf\ng\nh\ni\nj\nk\nl\nm\nn'" })
            .to_string(),
    });
    let _ = state.apply_command(TuiCommand::ToolExecuting {
        id: "tool-1".into(),
    });

    let assistant = state.transcript.root_blocks()[0];
    let assistant_block = state.transcript.block(assistant).expect("assistant root");
    let tool_use_id = assistant_block.children[0];
    let tool_use = state.transcript.block(tool_use_id).expect("tool use");
    assert!(tool_use.children.is_empty());

    let _ = state.apply_command(TuiCommand::ToolResult {
        id: "tool-1".into(),
        name: "bash".into(),
        content: vec![ContentBlock::Text {
            text: (1..=14)
                .map(|i| format!("line {i}"))
                .collect::<Vec<_>>()
                .join("\n"),
        }],
        details: None,
        artifact_path: None,
        is_error: false,
    });
    let tool_use = state
        .transcript
        .block(tool_use_id)
        .expect("tool use after result");
    let tool_result = state
        .transcript
        .block(tool_use.children[0])
        .expect("tool result after result");
    assert!(tool_result.content.contains("line 1"));
    assert!(tool_result.content.contains("line 3"));
    assert!(
        tool_result
            .content
            .contains(crate::ui_hints::TOOL_EXPAND_HINT)
    );
    assert!(!tool_result.content.contains("line 14"));

    state.mode = TuiMode::Transcript;
    state.focused_block = Some(tool_use_id);
    state.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    let tool_use = state
        .transcript
        .block(tool_use_id)
        .expect("tool use after expand");
    let tool_result = state
        .transcript
        .block(tool_use.children[0])
        .expect("tool result after expand");
    assert!(
        !tool_result
            .content
            .contains(crate::ui_hints::TOOL_EXPAND_HINT)
    );
    assert!(tool_result.content.contains("line 14"));
}
