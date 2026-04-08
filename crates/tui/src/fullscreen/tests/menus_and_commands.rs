use super::*;

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
