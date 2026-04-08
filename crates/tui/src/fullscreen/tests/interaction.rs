use super::common::*;
use super::*;

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
