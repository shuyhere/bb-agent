use super::types::FullscreenNoteLevel;
use std::time::{Duration, Instant};

use bb_core::types::ContentBlock;
use bb_session::{store::EntryRow, tree::TreeNode};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

use crate::select_list::SelectItem;

use super::{
    frame::build_frame,
    layout::Size,
    runtime::FullscreenState,
    scheduler::RenderScheduler,
    tool_format::{format_tool_call_content, format_tool_call_title, format_tool_result_content},
    transcript::{BlockId, BlockKind, NewBlock, Transcript},
    types::{
        FullscreenAppConfig, FullscreenCommand, FullscreenMode, FullscreenSubmission,
        HistoricalToolState,
    },
};

fn sample_state() -> (FullscreenState, BlockId, BlockId, BlockId) {
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

fn scrolling_state() -> (FullscreenState, Vec<BlockId>) {
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

fn screen_row_for_header(state: &FullscreenState, block_id: BlockId) -> u16 {
    let header_row = state
        .projection
        .header_row_for_block(block_id)
        .expect("header row should exist");
    let local_row = header_row.saturating_sub(state.viewport.viewport_top);
    let layout = state.current_layout();
    layout.transcript.y + local_row as u16
}

fn screen_row_for_first_content(state: &FullscreenState, block_id: BlockId) -> u16 {
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

#[test]
fn frame_renders_header_title_when_space_allows() {
    let mut state = FullscreenState::new(
        FullscreenAppConfig {
            title: "BB-Agent v0.1.0".to_string(),
            ..FullscreenAppConfig::default()
        },
        Size {
            width: 80,
            height: 12,
        },
    );
    state.prepare_for_render();
    let frame = build_frame(&state);

    assert!(frame.lines[0].contains("BB-Agent v0.1.0"));
    assert!(frame.lines[1].contains("Ctrl-C exit"));
}

#[test]
fn header_and_input_borders_follow_fullscreen_color_theme() {
    let mut state = FullscreenState::new(
        FullscreenAppConfig {
            title: "BB-Agent v0.1.0".to_string(),
            ..FullscreenAppConfig::default()
        },
        Size {
            width: 80,
            height: 12,
        },
    );
    state.color_theme = super::spinner::ColorTheme::Lavender;
    state
        .spinner
        .set_color_theme(super::spinner::ColorTheme::Lavender);
    state.prepare_for_render();
    let frame = build_frame(&state);
    let layout = state.current_layout();
    let title_color = state.color_theme.title_escape();
    let border_color = state.color_theme.border_escape();

    assert!(frame.lines[0].contains(&title_color));
    assert!(frame.lines[layout.input.y as usize].contains(&border_color));
    assert!(
        frame.lines[(layout.input.y + layout.input.height - 1) as usize].contains(&border_color)
    );
}

#[test]
fn ctrl_o_enters_tool_expand_mode_in_terminal_fallbacks() {
    let (mut state, _, tool, _) = sample_state();

    state.on_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL));
    assert_eq!(state.mode, FullscreenMode::Transcript);
    assert_eq!(state.focused_block, Some(tool));
    assert!(state.status_line.contains("selected:"));
}

#[test]
fn ctrl_shift_o_and_escape_switch_modes_and_clear_input() {
    let (mut state, _, _, _) = sample_state();

    state.on_key(KeyEvent::new(
        KeyCode::Char('O'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    ));
    assert_eq!(state.mode, FullscreenMode::Transcript);
    assert!(state.focused_block.is_some());
    assert!(!state.should_quit);

    // Esc from transcript returns to Normal
    state.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(state.mode, FullscreenMode::Normal);
    assert!(!state.should_quit);

    // Esc in Normal with empty input does NOT quit (shows hint)
    state.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(!state.should_quit);
    assert!(state.status_line.contains("Ctrl+C"));

    // Esc in Normal with text clears input
    state.input = "hello".to_string();
    state.cursor = 5;
    state.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(state.input.is_empty());
    assert!(!state.should_quit);

    // Ctrl+C quits
    state.on_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
    assert!(state.should_quit);
}

#[test]
fn ctrl_m_and_ctrl_j_toggle_expansion_in_tool_expand_mode() {
    let (mut state, _intro, tool, _) = sample_state();

    state.on_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL));
    assert_eq!(state.focused_block, Some(tool));

    state.on_key(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::CONTROL));
    assert!(state.expanded_tool_blocks.contains(&tool));

    state.on_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL));
    assert!(!state.expanded_tool_blocks.contains(&tool));
}

#[test]
fn transcript_keys_navigate_and_toggle_expansion() {
    let (mut state, _intro, tool, _) = sample_state();

    state.on_key(KeyEvent::new(
        KeyCode::Char('O'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    ));
    assert_eq!(state.focused_block, Some(tool));

    // Enter on a tool block toggles its expansion via expanded_tool_blocks
    state.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(state.expanded_tool_blocks.contains(&tool));

    // Enter again collapses it
    state.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(!state.expanded_tool_blocks.contains(&tool));
}

#[test]
fn transcript_toggle_expands_only_focused_tool_block() {
    let mut transcript = Transcript::new();
    let assistant = transcript.append_root_block(
        NewBlock::new(BlockKind::AssistantMessage, "assistant").with_content("done"),
    );
    let tool1 = transcript
        .append_child_block(
            assistant,
            NewBlock::new(BlockKind::ToolUse, "Read(/tmp/a.txt) • done").with_expandable(true),
        )
        .expect("tool1");
    let _ = transcript
        .append_child_block(
            tool1,
            NewBlock::new(BlockKind::ToolResult, "output")
                .with_content("Read 1 file (click or use Ctrl+Shift+O to enter tool expand mode)"),
        )
        .expect("result1");
    let tool2 = transcript
        .append_child_block(
            assistant,
            NewBlock::new(BlockKind::ToolUse, "Read(/tmp/b.txt) • done").with_expandable(true),
        )
        .expect("tool2");
    let _ = transcript
        .append_child_block(
            tool2,
            NewBlock::new(BlockKind::ToolResult, "output")
                .with_content("Read 1 file (click or use Ctrl+Shift+O to enter tool expand mode)"),
        )
        .expect("result2");

    let mut state = FullscreenState::new(
        FullscreenAppConfig {
            transcript,
            ..FullscreenAppConfig::default()
        },
        Size {
            width: 100,
            height: 24,
        },
    );
    state.on_key(KeyEvent::new(
        KeyCode::Char('O'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    ));
    state.focused_block = Some(tool1);
    state.sync_focus_tracking();

    state.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert!(state.expanded_tool_blocks.contains(&tool1));
    assert!(!state.expanded_tool_blocks.contains(&tool2));
}

#[test]
fn mouse_click_on_header_toggles_block() {
    let (mut state, _, tool, _) = sample_state();
    let screen_row = screen_row_for_header(&state, tool);

    state.on_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 0,
        row: screen_row,
        modifiers: KeyModifiers::NONE,
    });

    // Mouse click toggles tool block expansion, stays in Normal mode
    assert_eq!(state.mode, FullscreenMode::Normal);
    // Tool blocks use expanded_tool_blocks set, not transcript collapsed flag
    assert!(state.expanded_tool_blocks.contains(&tool));
}

#[test]
fn mouse_click_on_tool_result_row_toggles_parent_tool_block() {
    let (mut state, _, tool, result) = sample_state();
    let screen_row = screen_row_for_first_content(&state, result);

    state.on_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 0,
        row: screen_row,
        modifiers: KeyModifiers::NONE,
    });

    assert_eq!(state.mode, FullscreenMode::Normal);
    assert!(state.expanded_tool_blocks.contains(&tool));
}

#[test]
fn search_step_moves_focus_to_matching_block() {
    let (mut state, intro, _, result) = sample_state();

    state.on_key(KeyEvent::new(
        KeyCode::Char('O'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    ));
    state.focused_block = Some(intro);
    state.sync_focus_tracking();
    state.search.query = "world".to_string();
    state.search_step(true);

    assert_eq!(state.focused_block, Some(result));
}

#[test]
fn streaming_updates_do_not_force_auto_follow_back_to_bottom() {
    let mut transcript = Transcript::new();
    for idx in 0..8 {
        transcript.append_root_block(
            NewBlock::new(BlockKind::SystemNote, format!("note {idx}"))
                .with_content(format!("line {idx}")),
        );
    }

    let mut state = FullscreenState::new(
        FullscreenAppConfig {
            transcript,
            ..FullscreenAppConfig::default()
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

    let _ = state.apply_command(FullscreenCommand::TurnStart { turn_index: 0 });
    let _ = state.apply_command(FullscreenCommand::TextDelta("hello".to_string()));
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

    let mut state = FullscreenState::new(
        FullscreenAppConfig {
            transcript,
            ..FullscreenAppConfig::default()
        },
        Size {
            width: 80,
            height: 12,
        },
    );
    state.mode = FullscreenMode::Transcript;
    state.focused_block = Some(first);
    state.viewport.jump_to_top();
    state.viewport.auto_follow = false;
    state.sync_focus_tracking();
    let anchor_before = state
        .viewport
        .capture_header_anchor(&state.projection, first)
        .expect("anchor should exist");

    let _ = state.apply_command(FullscreenCommand::TurnStart { turn_index: 0 });
    let _ = state.apply_command(FullscreenCommand::TextDelta("delta".into()));
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
    let mut state = FullscreenState::new(
        FullscreenAppConfig::default(),
        Size {
            width: 80,
            height: 24,
        },
    );

    let _ = state.apply_command(FullscreenCommand::TurnStart { turn_index: 0 });
    let _ = state.apply_command(FullscreenCommand::ThinkingDelta("thinking".into()));
    let _ = state.apply_command(FullscreenCommand::ToolCallStart {
        id: "tool-1".into(),
        name: "bash".into(),
    });
    let _ = state.apply_command(FullscreenCommand::ToolCallDelta {
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

    let _ = state.apply_command(FullscreenCommand::ToolResult {
        id: "tool-1".into(),
        name: "bash".into(),
        content: vec![ContentBlock::Text {
            text: "file.txt".into(),
        }],
        details: None,
        artifact_path: None,
        is_error: false,
    });
    let _ = state.apply_command(FullscreenCommand::TextDelta("done".into()));
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

    let mut state = FullscreenState::new(
        FullscreenAppConfig::default(),
        Size {
            width: 80,
            height: 24,
        },
    );
    let _ = state.apply_command(FullscreenCommand::SetTranscriptWithToolStates {
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
            .contains("more lines; click or use Ctrl+Shift+O to enter tool expand mode")
    );
    assert!(tool_use.title.contains("Bash"));
}

#[test]
fn tool_result_appears_only_when_finished_and_enter_expands_output() {
    let mut state = FullscreenState::new(
        FullscreenAppConfig::default(),
        Size {
            width: 80,
            height: 24,
        },
    );

    let _ = state.apply_command(FullscreenCommand::TurnStart { turn_index: 0 });
    let _ = state.apply_command(FullscreenCommand::ToolCallStart {
        id: "tool-1".into(),
        name: "bash".into(),
    });
    let _ = state.apply_command(FullscreenCommand::ToolCallDelta {
        id: "tool-1".into(),
        args: serde_json::json!({ "command": "printf 'a\\nb\\nc\\nd\\ne\\nf\\ng\\nh\\ni\\nj\\nk\\nl\\nm\\nn'" }).to_string(),
    });
    let _ = state.apply_command(FullscreenCommand::ToolExecuting {
        id: "tool-1".into(),
    });

    let assistant = state.transcript.root_blocks()[0];
    let assistant_block = state.transcript.block(assistant).expect("assistant root");
    let tool_use_id = assistant_block.children[0];
    let tool_use = state.transcript.block(tool_use_id).expect("tool use");
    assert!(tool_use.children.is_empty());

    let _ = state.apply_command(FullscreenCommand::ToolResult {
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
            .contains("more lines; click or use Ctrl+Shift+O to enter tool expand mode")
    );
    assert!(!tool_result.content.contains("line 14"));

    state.mode = FullscreenMode::Transcript;
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
            .contains("more lines; click or use Ctrl+Shift+O to enter tool expand mode")
    );
    assert!(tool_result.content.contains("line 14"));
}

#[test]
fn ctrl_shift_o_focuses_latest_visible_tool_use_header() {
    let mut transcript = Transcript::new();
    let assistant = transcript.append_root_block(
        NewBlock::new(BlockKind::AssistantMessage, "assistant").with_content("done"),
    );
    let tool1 = transcript
        .append_child_block(
            assistant,
            NewBlock::new(BlockKind::ToolUse, "Read(/tmp/a.txt) • done").with_expandable(true),
        )
        .expect("tool1");
    let _ = transcript
        .append_child_block(
            tool1,
            NewBlock::new(BlockKind::ToolResult, "output").with_content("a"),
        )
        .expect("result1");
    let tool2 = transcript
        .append_child_block(
            assistant,
            NewBlock::new(BlockKind::ToolUse, "Read(/tmp/b.txt) • done").with_expandable(true),
        )
        .expect("tool2");
    let _ = transcript
        .append_child_block(
            tool2,
            NewBlock::new(BlockKind::ToolResult, "output").with_content("b"),
        )
        .expect("result2");
    let _tail = transcript.append_root_block(
        NewBlock::new(BlockKind::AssistantMessage, "assistant").with_content("summary"),
    );

    let mut state = FullscreenState::new(
        FullscreenAppConfig {
            transcript,
            ..FullscreenAppConfig::default()
        },
        Size {
            width: 80,
            height: 24,
        },
    );

    state.on_key(KeyEvent::new(
        KeyCode::Char('O'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    ));

    assert_eq!(state.mode, FullscreenMode::Transcript);
    assert_eq!(state.focused_block, Some(tool2));
    assert!(!state.viewport.auto_follow);
}

#[test]
fn scheduler_batches_streaming_bursts_until_idle_or_frame_cap() {
    let start = Instant::now();
    let mut scheduler = RenderScheduler::new(Duration::from_millis(30), Duration::from_millis(10));

    scheduler.mark_dirty(start);
    scheduler.mark_dirty(start + Duration::from_millis(8));
    scheduler.mark_dirty(start + Duration::from_millis(16));

    assert!(!scheduler.should_flush(start + Duration::from_millis(24)));
    assert!(scheduler.should_flush(start + Duration::from_millis(26)));

    scheduler.on_flushed();
    scheduler.mark_dirty(start + Duration::from_millis(40));
    assert!(scheduler.should_flush(start + Duration::from_millis(70)));
}

#[test]
fn scroll_events_toggle_follow_but_stay_in_normal_mode() {
    let (mut state, _) = scrolling_state();
    let transcript_row = state.current_layout().transcript.y;

    state.on_mouse(MouseEvent {
        kind: MouseEventKind::ScrollUp,
        column: 0,
        row: transcript_row,
        modifiers: KeyModifiers::NONE,
    });

    // Scroll does NOT enter Transcript mode — user stays in Normal
    assert_eq!(state.mode, FullscreenMode::Normal);
    assert!(!state.viewport.auto_follow);

    for _ in 0..10 {
        state.on_mouse(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 0,
            row: transcript_row,
            modifiers: KeyModifiers::NONE,
        });
        if state.viewport.auto_follow {
            break;
        }
    }

    assert!(state.viewport.auto_follow);
    assert_eq!(state.mode, FullscreenMode::Normal);
}

#[test]
fn ctrl_j_submits_like_enter_in_normal_mode() {
    let mut state = FullscreenState::new(
        FullscreenAppConfig::default(),
        Size {
            width: 80,
            height: 24,
        },
    );
    state.input = "hello".to_string();
    state.cursor = state.input.len();

    state.on_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL));

    assert!(state.input.is_empty());
    assert_eq!(state.submitted_inputs, vec!["hello".to_string()]);
    assert_eq!(state.status_line, "Working...");
}

#[test]
fn edit_tool_result_prefers_diff_when_available() {
    let rendered = format_tool_result_content(
        "edit",
        &[],
        Some(serde_json::json!({
            "path": "/tmp/demo.txt",
            "applied": 1,
            "total": 1,
            "diff": "@@ -1 +1 @@\n-old\n+new"
        })),
        None,
        false,
        false,
    );

    assert!(rendered.contains("applied 1/1 edit(s) to /tmp/demo.txt"));
    assert!(rendered.contains("@@ -1 +1 @@"));
    assert!(rendered.contains("-old"));
    assert!(rendered.contains("+new"));
}

#[test]
fn tool_titles_and_results_shorten_home_paths() {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp/test-home".to_string());
    let path = format!("{home}/project/demo.txt");
    let raw_args = serde_json::json!({ "path": path }).to_string();

    let title = format_tool_call_title("read", &raw_args);
    assert!(title.contains("~/project/demo.txt") || title.contains("/project/demo.txt"));

    let rendered = format_tool_result_content(
        "write",
        &[],
        Some(serde_json::json!({
            "path": format!("{home}/project/demo.txt"),
            "bytes": 12
        })),
        None,
        false,
        false,
    );
    assert!(
        rendered.contains("wrote 12 bytes to ~/project/demo.txt")
            || rendered.contains("wrote 12 bytes to /tmp/test-home/project/demo.txt")
    );
}

#[test]
fn tool_titles_include_interactive_context_details() {
    let read = format_tool_call_title(
        "read",
        &serde_json::json!({
            "path": "/tmp/demo.txt",
            "offset": 5,
            "limit": 3
        })
        .to_string(),
    );
    assert_eq!(read, "Read(/tmp/demo.txt:5-7)");

    let ls = format_tool_call_title(
        "ls",
        &serde_json::json!({
            "path": "/tmp",
            "limit": 25
        })
        .to_string(),
    );
    assert_eq!(ls, "LS(/tmp limit=25)");

    let grep = format_tool_call_title(
        "grep",
        &serde_json::json!({
            "pattern": "todo",
            "path": "/tmp/project",
            "glob": "*.rs"
        })
        .to_string(),
    );
    assert_eq!(grep, "Grep(/todo/ /tmp/project *.rs)");

    let find = format_tool_call_title(
        "find",
        &serde_json::json!({
            "pattern": "*.md",
            "path": "/tmp/project"
        })
        .to_string(),
    );
    assert_eq!(find, "Find(*.md /tmp/project)");

    let web_search = format_tool_call_title(
        "web_search",
        &serde_json::json!({
            "query": "Iran United States relations news today"
        })
        .to_string(),
    );
    assert_eq!(
        web_search,
        "WebSearch(\"Iran United States relations news today\")"
    );

    let web_fetch = format_tool_call_title(
        "web_fetch",
        &serde_json::json!({
            "url": "https://example.com/article"
        })
        .to_string(),
    );
    assert_eq!(web_fetch, "WebFetch(https://example.com/article)");

    let browser_fetch = format_tool_call_title(
        "browser_fetch",
        &serde_json::json!({
            "url": "https://example.com/protected"
        })
        .to_string(),
    );
    assert_eq!(browser_fetch, "BrowserFetch(https://example.com/protected)");
}

#[test]
fn bash_title_recovers_from_multiline_non_strict_json_args() {
    let raw = "{\"command\": \"cat > /tmp/cchistory_prompts/full_analysis.py << 'PYEOF'\nimport os\nprint('hi')\nPYEOF\"}";
    let title = format_tool_call_title("bash", raw);
    assert_eq!(
        title,
        "Bash(cat > /tmp/cchistory_prompts/full_analysis.py << 'PYEOF')"
    );
}

#[test]
fn artifact_paths_shorten_home_prefix() {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp/test-home".to_string());
    let rendered = format_tool_result_content(
        "write",
        &[],
        None,
        Some(format!("{home}/project/out.patch")),
        false,
        false,
    );
    assert!(
        rendered.contains("artifact: ~/project/out.patch")
            || rendered.contains("artifact: /tmp/test-home/project/out.patch")
    );
}

#[test]
fn write_and_edit_call_content_use_interactive_style_previews() {
    let write = format_tool_call_content(
        "write",
        &serde_json::json!({
            "path": "/tmp/demo.txt",
            "content": "one\ntwo\nthree\nfour\nfive\nsix"
        })
        .to_string(),
        false,
    );
    assert!(write.contains("one"));
    assert!(write.contains("three")); // 3rd line visible
    assert!(!write.contains("five")); // 5th line truncated at 3 lines
    assert!(write.contains("more lines; click or use Ctrl+Shift+O to enter tool expand mode"));
    assert!(!write.contains("\"content\""));

    let edit = format_tool_call_content(
        "edit",
        &serde_json::json!({
            "path": "/tmp/demo.txt",
            "edits": [
                { "oldText": "alpha", "newText": "beta" },
                { "oldText": "line1\nline2", "newText": "line1\nlineX" }
            ]
        })
        .to_string(),
        false,
    );
    assert!(edit.contains("2 edit block(s)"));
    assert!(edit.contains("1. - alpha"));
    assert!(edit.contains("+ beta"));
    assert!(edit.contains("line1\\nline2"));
    assert!(!edit.contains("\"oldText\""));
}

#[test]
fn tool_result_previews_use_interactive_limits_and_truncation() {
    let bash_lines = (1..=14)
        .map(|i| format!("line\t{i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let bash = format_tool_result_content(
        "bash",
        &[ContentBlock::Text {
            text: bash_lines.clone(),
        }],
        None,
        None,
        false,
        false,
    );
    assert!(bash.contains("line   1"));
    assert!(bash.contains("line   3"));
    assert!(bash.contains("more lines; click or use Ctrl+Shift+O to enter tool expand mode"));
    assert!(!bash.contains("line   4"));

    let grep_lines = (1..=16)
        .map(|i| format!("match {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let grep = format_tool_result_content(
        "grep",
        &[ContentBlock::Text {
            text: grep_lines.clone(),
        }],
        None,
        None,
        false,
        false,
    );
    assert!(grep.contains("match 1"));
    assert!(grep.contains("match 3"));
    assert!(grep.contains("more lines; click or use Ctrl+Shift+O to enter tool expand mode"));
    assert!(!grep.contains("match 4"));

    let expanded = format_tool_result_content(
        "bash",
        &[ContentBlock::Text { text: bash_lines }],
        None,
        None,
        false,
        true,
    );
    assert!(expanded.contains("line   14"));
    assert!(!expanded.contains("... (2 more lines; click or use Ctrl+Shift+O to enter tool expand mode)"));

    let long_lines = (1..=140)
        .map(|i| format!("tail {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let expanded_long = format_tool_result_content(
        "bash",
        &[ContentBlock::Text { text: long_lines }],
        None,
        None,
        false,
        true,
    );
    assert!(expanded_long.contains("… output truncated (21 lines hidden)"));
    assert!(expanded_long.contains("tail 1"));
    assert!(expanded_long.contains("tail 140"));
}

#[test]
fn collapsed_bash_preview_truncates_long_single_line_before_terminal_wrap() {
    let bash = format_tool_result_content(
        "bash",
        &[ContentBlock::Text {
            text: format!("{} tail-marker", "x".repeat(400)),
        }],
        None,
        None,
        false,
        false,
    );
    assert!(bash.contains('…'));
    assert!(!bash.contains("tail-marker"));
}

#[test]
fn typing_slash_in_normal_mode_shows_fullscreen_command_menu() {
    let mut state = FullscreenState::new(
        FullscreenAppConfig::default(),
        Size {
            width: 80,
            height: 20,
        },
    );

    state.on_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
    let lines = state
        .render_slash_menu_lines(80)
        .expect("slash menu should be visible");
    let joined = lines.join("\n");
    assert!(joined.contains("/model"));
    assert!(joined.contains("/copy"));
    assert!(state.requested_footer_height() >= 6);

    let first_line_plain = crate::utils::strip_ansi(&lines[0]);
    assert!(first_line_plain.starts_with("→ /"));
    assert!(!lines[0].contains("\x1b[7m"));

    state.on_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
    let lines = state
        .render_slash_menu_lines(80)
        .expect("filtered slash menu should be visible");
    let joined = lines.join("\n");
    assert!(joined.contains("/settings"));
}

#[test]
fn typing_at_in_normal_mode_shows_attach_menu_immediately() {
    let dir = std::env::temp_dir().join(format!(
        "bb-tui-at-menu-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time after epoch")
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("create temp test dir");
    std::fs::write(dir.join("notes.txt"), "hello").expect("write test file");

    let config = FullscreenAppConfig {
        cwd: dir.clone(),
        ..FullscreenAppConfig::default()
    };
    let mut state = FullscreenState::new(
        config,
        Size {
            width: 80,
            height: 20,
        },
    );

    state.on_key(KeyEvent::new(KeyCode::Char('@'), KeyModifiers::NONE));

    let lines = state
        .render_at_file_menu_lines(80)
        .expect("attach menu should be visible after bare @");
    let joined = lines.join("\n");
    assert!(joined.contains("notes.txt"), "menu should list cwd entries");

    let _ = std::fs::remove_file(dir.join("notes.txt"));
    let _ = std::fs::remove_dir(&dir);
}

#[test]
fn slash_menu_scrolls_when_selection_moves_past_visible_window() {
    let mut state = FullscreenState::new(
        FullscreenAppConfig::default(),
        Size {
            width: 80,
            height: 20,
        },
    );

    state.on_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
    for _ in 0..6 {
        state.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    }

    assert!(state.slash_menu.is_some());

    let joined = state
        .render_slash_menu_lines(80)
        .expect("slash menu should render")
        .join("\n");
    assert!(joined.contains("more above"));
    // After scrolling 6 items down, later commands should be visible
    assert!(joined.contains('/'), "menu should contain slash commands");
}

#[test]
fn enter_on_hidden_scrolled_slash_item_accepts_that_item() {
    let mut state = FullscreenState::new(
        FullscreenAppConfig::default(),
        Size {
            width: 80,
            height: 20,
        },
    );

    state.on_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
    for _ in 0..6 {
        state.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    }
    let expected = state
        .slash_menu
        .as_ref()
        .and_then(|menu| menu.selected_value())
        .expect("selected slash command");

    state.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(state.input, expected);
    assert!(state.slash_menu.is_none());
}

#[test]
fn tab_accepts_slash_menu_selection() {
    let mut state = FullscreenState::new(
        FullscreenAppConfig::default(),
        Size {
            width: 80,
            height: 20,
        },
    );

    state.on_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
    state.on_key(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE));
    state.on_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));

    assert_eq!(state.input, "/model");
    assert!(state.slash_menu.is_none());
}

#[test]
fn enter_submits_exact_slash_command_without_waiting_for_second_enter() {
    let mut state = FullscreenState::new(
        FullscreenAppConfig::default(),
        Size {
            width: 80,
            height: 20,
        },
    );

    for ch in "/model".chars() {
        state.on_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }
    state.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert!(state.input.is_empty());
    assert!(state.transcript.root_blocks().is_empty());
    assert!(state.status_line.is_empty());
    assert_eq!(
        state.take_pending_submissions(),
        vec![FullscreenSubmission::Input("/model".to_string())]
    );
}

#[test]
fn ctrl_j_submits_exact_slash_command_without_llm_send_path() {
    let mut state = FullscreenState::new(
        FullscreenAppConfig::default(),
        Size {
            width: 80,
            height: 20,
        },
    );

    for ch in "/settings".chars() {
        state.on_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }
    state.on_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL));

    assert!(state.input.is_empty());
    assert!(state.transcript.root_blocks().is_empty());
    assert!(state.status_line.is_empty());
    assert_eq!(
        state.take_pending_submissions(),
        vec![FullscreenSubmission::Input("/settings".to_string())]
    );
}

#[test]
fn enter_submits_argument_slash_command_without_prompt_echo_or_working() {
    let mut state = FullscreenState::new(
        FullscreenAppConfig::default(),
        Size {
            width: 80,
            height: 20,
        },
    );

    for ch in "/name demo".chars() {
        state.on_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }
    state.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert!(state.input.is_empty());
    assert!(state.transcript.root_blocks().is_empty());
    assert!(state.status_line.is_empty());
    assert_eq!(
        state.take_pending_submissions(),
        vec![FullscreenSubmission::Input("/name demo".to_string())]
    );
}

#[test]
fn select_menu_enter_emits_control_submission() {
    let mut state = FullscreenState::new(
        FullscreenAppConfig::default(),
        Size {
            width: 80,
            height: 20,
        },
    );

    let _ = state.apply_command(FullscreenCommand::OpenSelectMenu {
        menu_id: "model".to_string(),
        title: "Select model".to_string(),
        items: vec![
            SelectItem {
                label: "anthropic/claude".to_string(),
                detail: None,
                value: "anthropic/claude".to_string(),
            },
            SelectItem {
                label: "openai/gpt-4o".to_string(),
                detail: None,
                value: "openai/gpt-4o".to_string(),
            },
        ],
        selected_value: None,
    });
    state.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    state.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(
        state.take_pending_submissions(),
        vec![FullscreenSubmission::MenuSelection {
            menu_id: "model".to_string(),
            value: "openai/gpt-4o".to_string(),
        }]
    );
}

#[test]
fn open_select_menu_respects_selected_value() {
    let mut state = FullscreenState::new(
        FullscreenAppConfig::default(),
        Size {
            width: 80,
            height: 20,
        },
    );

    let _ = state.apply_command(FullscreenCommand::OpenSelectMenu {
        menu_id: "model".to_string(),
        title: "Select model".to_string(),
        items: vec![
            SelectItem {
                label: "anthropic/claude".to_string(),
                detail: None,
                value: "anthropic/claude".to_string(),
            },
            SelectItem {
                label: "openai/gpt-4o".to_string(),
                detail: None,
                value: "openai/gpt-4o".to_string(),
            },
        ],
        selected_value: Some("openai/gpt-4o".to_string()),
    });
    state.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(
        state.take_pending_submissions(),
        vec![FullscreenSubmission::MenuSelection {
            menu_id: "model".to_string(),
            value: "openai/gpt-4o".to_string(),
        }]
    );
}

#[test]
fn select_menu_ctrl_j_emits_control_submission() {
    let mut state = FullscreenState::new(
        FullscreenAppConfig::default(),
        Size {
            width: 80,
            height: 20,
        },
    );

    let _ = state.apply_command(FullscreenCommand::OpenSelectMenu {
        menu_id: "settings".to_string(),
        title: "Settings".to_string(),
        items: vec![SelectItem {
            label: "thinking".to_string(),
            detail: None,
            value: "thinking".to_string(),
        }],
        selected_value: None,
    });
    state.on_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL));

    assert_eq!(
        state.take_pending_submissions(),
        vec![FullscreenSubmission::MenuSelection {
            menu_id: "settings".to_string(),
            value: "thinking".to_string(),
        }]
    );
}

#[test]
fn tree_menu_enter_emits_control_submission() {
    let mut state = FullscreenState::new(
        FullscreenAppConfig::default(),
        Size {
            width: 80,
            height: 24,
        },
    );

    let tree = vec![TreeNode {
        entry_id: "root".to_string(),
        parent_id: None,
        entry_type: "message".to_string(),
        timestamp: "2026-01-01T00:00:00Z".to_string(),
        children: vec![TreeNode {
            entry_id: "child".to_string(),
            parent_id: Some("root".to_string()),
            entry_type: "message".to_string(),
            timestamp: "2026-01-01T00:01:00Z".to_string(),
            children: vec![],
        }],
    }];
    let entries = vec![
        EntryRow {
            session_id: "s".to_string(),
            seq: 1,
            entry_id: "root".to_string(),
            parent_id: None,
            entry_type: "message".to_string(),
            timestamp: "2026-01-01T00:00:00Z".to_string(),
            payload: r#"{"message":{"content":[{"text":"hello root"}],"timestamp":1}}"#
                .to_string(),
        },
        EntryRow {
            session_id: "s".to_string(),
            seq: 2,
            entry_id: "child".to_string(),
            parent_id: Some("root".to_string()),
            entry_type: "message".to_string(),
            timestamp: "2026-01-01T00:01:00Z".to_string(),
            payload: r#"{"message":{"content":[{"text":"hello child"}],"provider":"anthropic","timestamp":2}}"#
                .to_string(),
        },
    ];

    let _ = state.apply_command(FullscreenCommand::OpenTreeMenu {
        menu_id: "tree-entry".to_string(),
        title: "Session Tree".to_string(),
        tree,
        entries,
        active_leaf: Some("root".to_string()),
        selected_value: None,
    });
    state.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    state.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(
        state.take_pending_submissions(),
        vec![FullscreenSubmission::MenuSelection {
            menu_id: "tree-entry".to_string(),
            value: "child".to_string(),
        }]
    );
}

#[test]
fn tree_menu_ctrl_u_filters_to_user_messages() {
    let mut state = FullscreenState::new(
        FullscreenAppConfig::default(),
        Size {
            width: 80,
            height: 24,
        },
    );

    let tree = vec![TreeNode {
        entry_id: "u1".to_string(),
        parent_id: None,
        entry_type: "message".to_string(),
        timestamp: "2026-01-01T00:00:00Z".to_string(),
        children: vec![TreeNode {
            entry_id: "a1".to_string(),
            parent_id: Some("u1".to_string()),
            entry_type: "message".to_string(),
            timestamp: "2026-01-01T00:01:00Z".to_string(),
            children: vec![TreeNode {
                entry_id: "u2".to_string(),
                parent_id: Some("a1".to_string()),
                entry_type: "message".to_string(),
                timestamp: "2026-01-01T00:02:00Z".to_string(),
                children: vec![],
            }],
        }],
    }];
    let entries = vec![
        EntryRow { session_id: "s".to_string(), seq: 1, entry_id: "u1".to_string(), parent_id: None, entry_type: "message".to_string(), timestamp: "2026-01-01T00:00:00Z".to_string(), payload: r#"{"message":{"content":[{"text":"first user"}],"timestamp":1}}"#.to_string() },
        EntryRow { session_id: "s".to_string(), seq: 2, entry_id: "a1".to_string(), parent_id: Some("u1".to_string()), entry_type: "message".to_string(), timestamp: "2026-01-01T00:01:00Z".to_string(), payload: r#"{"message":{"content":[{"text":"assistant reply"}],"provider":"anthropic","timestamp":2}}"#.to_string() },
        EntryRow { session_id: "s".to_string(), seq: 3, entry_id: "u2".to_string(), parent_id: Some("a1".to_string()), entry_type: "message".to_string(), timestamp: "2026-01-01T00:02:00Z".to_string(), payload: r#"{"message":{"content":[{"text":"second user"}],"timestamp":3}}"#.to_string() },
    ];

    let _ = state.apply_command(FullscreenCommand::OpenTreeMenu {
        menu_id: "tree-entry".to_string(),
        title: "Session Tree".to_string(),
        tree,
        entries,
        active_leaf: Some("u2".to_string()),
        selected_value: None,
    });

    let before = state
        .render_tree_menu_lines(80)
        .unwrap_or_default()
        .join("\n");
    assert!(before.contains("assistant:"));

    state.on_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
    let after = state
        .render_tree_menu_lines(80)
        .unwrap_or_default()
        .join("\n");
    assert!(after.contains("user:"));
    assert!(!after.contains("assistant:"));
}

#[test]
fn tree_menu_bracket_fold_and_unfold_branch() {
    let mut state = FullscreenState::new(
        FullscreenAppConfig::default(),
        Size {
            width: 80,
            height: 24,
        },
    );

    let tree = vec![TreeNode {
        entry_id: "root".to_string(),
        parent_id: None,
        entry_type: "message".to_string(),
        timestamp: "2026-01-01T00:00:00Z".to_string(),
        children: vec![TreeNode {
            entry_id: "child".to_string(),
            parent_id: Some("root".to_string()),
            entry_type: "message".to_string(),
            timestamp: "2026-01-01T00:01:00Z".to_string(),
            children: vec![],
        }],
    }];
    let entries = vec![
        EntryRow { session_id: "s".to_string(), seq: 1, entry_id: "root".to_string(), parent_id: None, entry_type: "message".to_string(), timestamp: "2026-01-01T00:00:00Z".to_string(), payload: r#"{"message":{"content":[{"text":"root user"}],"timestamp":1}}"#.to_string() },
        EntryRow { session_id: "s".to_string(), seq: 2, entry_id: "child".to_string(), parent_id: Some("root".to_string()), entry_type: "message".to_string(), timestamp: "2026-01-01T00:01:00Z".to_string(), payload: r#"{"message":{"content":[{"text":"child assistant"}],"provider":"anthropic","timestamp":2}}"#.to_string() },
    ];

    let _ = state.apply_command(FullscreenCommand::OpenTreeMenu {
        menu_id: "tree-entry".to_string(),
        title: "Session Tree".to_string(),
        tree,
        entries,
        active_leaf: Some("child".to_string()),
        selected_value: Some("root".to_string()),
    });

    let before = state
        .render_tree_menu_lines(80)
        .unwrap_or_default()
        .join("\n");
    assert!(before.contains("child assistant"));

    state.on_key(KeyEvent::new(KeyCode::Char('['), KeyModifiers::NONE));
    let folded = state
        .render_tree_menu_lines(80)
        .unwrap_or_default()
        .join("\n");
    assert!(!folded.contains("child assistant"));

    state.on_key(KeyEvent::new(KeyCode::Char(']'), KeyModifiers::NONE));
    let unfolded = state
        .render_tree_menu_lines(80)
        .unwrap_or_default()
        .join("\n");
    assert!(unfolded.contains("child assistant"));
}

#[test]
fn updating_extra_slash_items_refreshes_open_slash_menu() {
    let mut state = FullscreenState::new(
        FullscreenAppConfig::default(),
        Size {
            width: 80,
            height: 20,
        },
    );

    state.input = "/skill:d".to_string();
    state.cursor = state.input.len();
    state.update_slash_menu();
    let before = state
        .render_slash_menu_lines(80)
        .unwrap_or_default()
        .join("\n");
    assert!(!before.contains("/skill:demo-review"));

    let _ = state.apply_command(FullscreenCommand::SetExtraSlashItems(vec![SelectItem {
        label: "/skill:demo-review".to_string(),
        detail: Some("Demo skill".to_string()),
        value: "/skill:demo-review".to_string(),
    }]));

    let after = state
        .render_slash_menu_lines(80)
        .unwrap_or_default()
        .join("\n");
    assert!(after.contains("/skill:demo-review"));
}

#[test]
fn keyboard_navigation_turns_follow_off_and_resize_preserves_focus_anchor_when_possible() {
    let mut transcript = Transcript::new();
    let assistant = transcript.append_root_block(
        NewBlock::new(BlockKind::AssistantMessage, "assistant").with_content("done"),
    );
    for idx in 0..8 {
        let tool = transcript
            .append_child_block(
                assistant,
                NewBlock::new(BlockKind::ToolUse, format!("Read(/tmp/{idx}.txt) • done"))
                    .with_expandable(true),
            )
            .expect("tool");
        let _ = transcript
            .append_child_block(
                tool,
                NewBlock::new(BlockKind::ToolResult, "output").with_content("hello"),
            )
            .expect("tool result");
    }
    let mut state = FullscreenState::new(
        FullscreenAppConfig {
            transcript,
            ..FullscreenAppConfig::default()
        },
        Size {
            width: 80,
            height: 20,
        },
    );

    state.on_key(KeyEvent::new(
        KeyCode::Char('O'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    ));
    assert!(!state.viewport.auto_follow);

    state.on_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    let focused = state.focused_block.expect("focus after navigation");
    let header_row = state
        .projection
        .header_row_for_block(focused)
        .expect("focused header row should exist");
    let anchor_offset = header_row.saturating_sub(state.viewport.viewport_top);

    assert!(!state.viewport.auto_follow);

    state.on_resize(72, 14);

    let resized_header_row = state
        .projection
        .header_row_for_block(focused)
        .expect("focused header row should still exist");
    let expected_top = resized_header_row
        .saturating_sub(anchor_offset)
        .min(state.viewport.bottom_top());
    assert_eq!(state.focused_block, Some(focused));
    assert!(state.viewport.viewport_top >= expected_top);
    assert!(resized_header_row >= state.viewport.viewport_top);
    assert!(resized_header_row < state.viewport.viewport_top + state.viewport.viewport_height);
}

#[test]
fn cursor_is_only_visible_in_normal_mode() {
    let (mut state, _, _, _) = sample_state();
    state.input = "hello".to_string();
    state.cursor = state.input.len();

    assert!(build_frame(&state).cursor.is_some());

    state.on_key(KeyEvent::new(
        KeyCode::Char('O'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    ));
    assert_eq!(state.mode, FullscreenMode::Transcript);
    assert!(build_frame(&state).cursor.is_none());
}

#[test]
fn turn_end_returns_to_normal_mode_when_auto_following() {
    let mut state = FullscreenState::new(
        FullscreenAppConfig::default(),
        Size {
            width: 80,
            height: 24,
        },
    );
    // Start a turn
    let _ = state.apply_command(FullscreenCommand::TurnStart { turn_index: 0 });
    assert!(state.has_active_turn());
    assert_eq!(state.mode, FullscreenMode::Normal);

    // End the turn — should stay in Normal mode
    let _ = state.apply_command(FullscreenCommand::TurnEnd);
    assert!(!state.has_active_turn());
    assert_eq!(state.mode, FullscreenMode::Normal);
    assert!(
        state.status_line.trim().is_empty(),
        "status should be cleared"
    );
}

#[test]
fn turn_end_stays_in_transcript_when_user_scrolled_away() {
    let mut state = FullscreenState::new(
        FullscreenAppConfig::default(),
        Size {
            width: 80,
            height: 24,
        },
    );
    let _ = state.apply_command(FullscreenCommand::TurnStart { turn_index: 0 });

    // User scrolls up → enters Transcript mode, auto_follow off
    state.mode = FullscreenMode::Transcript;
    state.viewport.auto_follow = false;

    let _ = state.apply_command(FullscreenCommand::TurnEnd);
    // Should stay in Transcript since user explicitly scrolled away
    assert_eq!(state.mode, FullscreenMode::Transcript);
}

#[test]
fn escape_from_transcript_returns_to_normal() {
    let mut state = FullscreenState::new(
        FullscreenAppConfig::default(),
        Size {
            width: 80,
            height: 24,
        },
    );
    // Enter tool expand mode via Ctrl+Shift+O
    state.on_key(KeyEvent::new(
        KeyCode::Char('O'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    ));
    assert_eq!(state.mode, FullscreenMode::Transcript);

    // Escape returns to Normal
    state.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(state.mode, FullscreenMode::Normal);
    assert!(state.viewport.auto_follow);
}

#[test]
fn ctrl_y_toggles_selection_mode_without_leaving_fullscreen() {
    let mut state = FullscreenState::new(
        FullscreenAppConfig::default(),
        Size {
            width: 80,
            height: 24,
        },
    );

    state.on_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::CONTROL));
    assert!(state.selection_mode);

    state.on_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::CONTROL));
    assert!(!state.selection_mode);
}

#[test]
fn dragging_selection_copies_character_ranges_without_exiting_fullscreen() {
    let mut transcript = Transcript::new();
    let message = transcript.append_root_block(
        NewBlock::new(BlockKind::AssistantMessage, "assistant").with_content("abcdef"),
    );
    let mut state = FullscreenState::new(
        FullscreenAppConfig {
            transcript,
            ..FullscreenAppConfig::default()
        },
        Size {
            width: 80,
            height: 24,
        },
    );
    state.selection_mode = true;
    let row = screen_row_for_first_content(&state, message);

    state.on_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 1,
        row,
        modifiers: KeyModifiers::NONE,
    });
    state.on_mouse(MouseEvent {
        kind: MouseEventKind::Drag(MouseButton::Left),
        column: 3,
        row,
        modifiers: KeyModifiers::NONE,
    });
    state.on_mouse(MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column: 3,
        row,
        modifiers: KeyModifiers::NONE,
    });

    let copied = state
        .take_pending_clipboard_copy()
        .expect("selection should copy text");
    assert_eq!(copied, "bcd");
    assert_eq!(state.mode, FullscreenMode::Normal);
}

#[test]
fn selection_highlight_is_character_precise_not_full_row() {
    let mut transcript = Transcript::new();
    transcript.append_root_block(
        NewBlock::new(BlockKind::AssistantMessage, "assistant").with_content("abcdef"),
    );
    let mut state = FullscreenState::new(
        FullscreenAppConfig {
            transcript,
            ..FullscreenAppConfig::default()
        },
        Size {
            width: 40,
            height: 12,
        },
    );
    state.selection_mode = true;
    state.selection_anchor_row = Some(0);
    state.selection_anchor_col = Some(1);
    state.selection_focus_row = Some(0);
    state.selection_focus_col = Some(3);
    state.prepare_for_render();

    let frame = build_frame(&state);
    let transcript_line = &frame.lines[state.current_layout().transcript.y as usize];
    let plain = crate::utils::strip_ansi(transcript_line);

    assert!(plain.starts_with("abcdef"));
    assert!(transcript_line.contains("a\x1b[7mbcd\x1b[0m"));
    assert!(!transcript_line.contains("\x1b[7mabcdef"));
}

#[test]
fn mouse_scroll_does_not_enter_transcript_mode() {
    let (mut state, _) = scrolling_state();
    assert_eq!(state.mode, FullscreenMode::Normal);

    // Scroll up — should scroll viewport but stay in Normal mode
    state.on_mouse(MouseEvent {
        kind: MouseEventKind::ScrollUp,
        column: 10,
        row: 5,
        modifiers: KeyModifiers::NONE,
    });
    assert_eq!(
        state.mode,
        FullscreenMode::Normal,
        "scroll should not enter transcript"
    );
    assert!(
        !state.viewport.auto_follow,
        "auto_follow should be off after scroll up"
    );
}

#[test]
fn push_note_creates_visible_content_in_frame() {
    let mut state = FullscreenState::new(
        FullscreenAppConfig::default(),
        Size {
            width: 80,
            height: 24,
        },
    );
    let _ = state.apply_command(FullscreenCommand::PushNote {
        level: FullscreenNoteLevel::Status,
        text: "[Skills]\n  /skill:demo-review\n    ~/skills/demo/SKILL.md".to_string(),
    });
    state.prepare_for_render();
    let frame = build_frame(&state);
    let all_text = frame.lines.join("\n");
    assert!(
        all_text.contains("Skills"),
        "frame should show Skills header: got {:?}",
        &frame.lines[..5.min(frame.lines.len())]
    );
    assert!(
        all_text.contains("/skill:demo-review"),
        "frame should show skill name"
    );
}
