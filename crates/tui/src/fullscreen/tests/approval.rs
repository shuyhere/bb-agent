use super::*;

#[test]
fn approval_dialog_enter_submits_selected_decision() {
    let mut state = FullscreenState::new(
        FullscreenAppConfig::default(),
        Size {
            width: 80,
            height: 24,
        },
    );

    let _ = state.apply_command(FullscreenCommand::OpenApprovalDialog(
        FullscreenApprovalDialog {
            title: "Approval required".to_string(),
            command: "cargo check --workspace".to_string(),
            reason: "Command is not in the read-only allowlist".to_string(),
            lines: vec![],
            allow_session: true,
            session_scope_label: Some("commands that start with `cargo check`".to_string()),
            deny_input: String::new(),
            deny_cursor: 0,
            deny_input_placeholder: Some("Tell BB what to do differently".to_string()),
            selected: FullscreenApprovalChoice::ApproveOnce,
        },
    ));
    state.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    state.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(
        state.take_pending_submissions(),
        vec![FullscreenSubmission::ApprovalDecision {
            choice: FullscreenApprovalChoice::ApproveForSession,
            steer_message: None,
        }]
    );
}

#[test]
fn approval_dialog_escape_denies_without_extra_navigation() {
    let mut state = FullscreenState::new(
        FullscreenAppConfig::default(),
        Size {
            width: 80,
            height: 24,
        },
    );

    let _ = state.apply_command(FullscreenCommand::OpenApprovalDialog(
        FullscreenApprovalDialog {
            title: "Approval required".to_string(),
            command: "git checkout -b feature".to_string(),
            reason: "Command may change repository state".to_string(),
            lines: vec![],
            allow_session: true,
            session_scope_label: Some("commands that start with `git checkout`".to_string()),
            deny_input: String::new(),
            deny_cursor: 0,
            deny_input_placeholder: Some("Tell BB what to do differently".to_string()),
            selected: FullscreenApprovalChoice::ApproveOnce,
        },
    ));
    state.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

    assert_eq!(
        state.take_pending_submissions(),
        vec![FullscreenSubmission::ApprovalDecision {
            choice: FullscreenApprovalChoice::Deny,
            steer_message: None,
        }]
    );
}

#[test]
fn approval_dialog_deny_can_capture_steer_message() {
    let mut state = FullscreenState::new(
        FullscreenAppConfig::default(),
        Size {
            width: 80,
            height: 24,
        },
    );

    let _ = state.apply_command(FullscreenCommand::OpenApprovalDialog(
        FullscreenApprovalDialog {
            title: "Approval required for bash command".to_string(),
            command: "echo hi > /tmp/out.txt".to_string(),
            reason: "Command uses shell control operators, redirection, or substitution"
                .to_string(),
            lines: vec![],
            allow_session: true,
            session_scope_label: Some("`echo hi > /tmp/out.txt`".to_string()),
            deny_input: String::new(),
            deny_cursor: 0,
            deny_input_placeholder: Some("Tell BB what to do differently".to_string()),
            selected: FullscreenApprovalChoice::ApproveOnce,
        },
    ));
    state.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    state.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    for ch in "Use rg instead".chars() {
        state.on_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }
    state.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(
        state.take_pending_submissions(),
        vec![FullscreenSubmission::ApprovalDecision {
            choice: FullscreenApprovalChoice::Deny,
            steer_message: Some("Use rg instead".to_string()),
        }]
    );
}

#[test]
fn approval_dialog_renders_command_reason_and_buttons() {
    let mut state = FullscreenState::new(
        FullscreenAppConfig::default(),
        Size {
            width: 80,
            height: 24,
        },
    );

    let _ = state.apply_command(FullscreenCommand::OpenApprovalDialog(
        FullscreenApprovalDialog {
            title: "Approval required for bash command".to_string(),
            command: "echo hi > /tmp/out.txt".to_string(),
            reason: "Command uses shell control operators, redirection, or substitution"
                .to_string(),
            lines: vec![],
            allow_session: true,
            session_scope_label: Some("`echo hi > /tmp/out.txt`".to_string()),
            deny_input: String::new(),
            deny_cursor: 0,
            deny_input_placeholder: Some("Tell BB what to do differently".to_string()),
            selected: FullscreenApprovalChoice::ApproveOnce,
        },
    ));
    state.prepare_for_render();
    let frame = build_frame(&state);
    let rendered = frame.lines.join("\n");

    assert!(rendered.contains("Approval required for bash command"));
    assert!(rendered.contains("echo hi > /tmp/out.txt"));
    assert!(rendered.contains("Approval required for bash command"));
    assert!(rendered.contains("→ Yes, proceed [y]"));
    assert!(rendered.contains("don't ask again for `echo hi > /tmp/out.txt` in this session"));
    assert!(rendered.contains("No, and tell BB what to do differently [n]"));
    assert!(frame.cursor.is_none());
}
