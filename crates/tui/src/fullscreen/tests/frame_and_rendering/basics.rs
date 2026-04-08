use super::super::common::*;
use super::super::*;

fn make_attachment_test_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "bb-tui-attachment-chips-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time after epoch")
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("create temp test dir");
    dir
}

#[test]
fn frame_renders_header_title_when_space_allows() {
    let mut state = FullscreenState::new(
        FullscreenAppConfig {
            title: "♡ BB-Agent v0.1.0".to_string(),
            ..FullscreenAppConfig::default()
        },
        Size {
            width: 80,
            height: 12,
        },
    );
    state.prepare_for_render();
    let frame = build_frame(&state);

    assert!(frame.lines[0].contains("♡ BB-Agent v0.1.0"));
    assert!(frame.lines[1].contains("Ctrl-C exit"));
}

#[test]
fn frame_renders_attachment_chips_for_pending_and_at_files() {
    let dir = make_attachment_test_dir();
    let image = dir.join("preview image.png");
    let doc = dir.join("notes.pdf");
    std::fs::write(&image, b"png-bytes").expect("write image file");
    std::fs::write(&doc, b"pdf-bytes").expect("write doc file");

    let mut state = FullscreenState::new(
        FullscreenAppConfig {
            cwd: dir.clone(),
            ..FullscreenAppConfig::default()
        },
        Size {
            width: 80,
            height: 20,
        },
    );
    state.pending_image_paths.push(image.display().to_string());
    state.input = "@\"notes.pdf\" summarize this".to_string();
    state.cursor = state.input.len();
    state.prepare_for_render();

    let frame = build_frame(&state);
    let joined = frame.lines.join("\n");
    assert!(
        joined.contains("[preview image.png, 1KB]"),
        "frame did not contain pending image chip:\n{joined}"
    );
    assert!(
        joined.contains("[notes.pdf, 1KB]"),
        "frame did not contain @file chip:\n{joined}"
    );

    let _ = std::fs::remove_file(image);
    let _ = std::fs::remove_file(doc);
    let _ = std::fs::remove_dir(dir);
}

#[test]
fn header_and_input_borders_follow_fullscreen_color_theme() {
    let mut state = FullscreenState::new(
        FullscreenAppConfig {
            title: "♡ BB-Agent v0.1.0".to_string(),
            ..FullscreenAppConfig::default()
        },
        Size {
            width: 80,
            height: 12,
        },
    );
    state.color_theme = crate::fullscreen::spinner::ColorTheme::Lavender;
    state
        .spinner
        .set_color_theme(crate::fullscreen::spinner::ColorTheme::Lavender);
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

    state.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(state.mode, FullscreenMode::Normal);
    assert!(!state.should_quit);

    state.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(!state.should_quit);
    assert!(state.status_line.contains("Ctrl+C"));

    state.input = "hello".to_string();
    state.cursor = 5;
    state.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(state.input.is_empty());
    assert!(!state.should_quit);

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

    state.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(state.expanded_tool_blocks.contains(&tool));

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

    assert_eq!(state.mode, FullscreenMode::Normal);
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
