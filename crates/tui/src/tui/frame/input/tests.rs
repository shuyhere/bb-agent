use std::time::{SystemTime, UNIX_EPOCH};

use crate::tui::layout::Size;
use crate::tui::runtime::TuiState;
use crate::tui::types::{TuiAppConfig, TuiApprovalChoice, TuiApprovalDialog, TuiAuthDialog};

use super::*;

fn unique_temp_file(name: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time should move forward")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("bb-tui-{name}-{nanos}.txt"));
    std::fs::write(&path, "demo").expect("temp file should be writable");
    path
}

#[test]
fn visible_input_text_elides_attachment_tokens_and_keeps_cursor_on_the_gap() {
    let file = unique_temp_file("visible-input");
    let input = format!("open @{path} please", path = file.display());
    let cursor = input.find('@').expect("attachment token") + 3;

    let (visible, mapped_cursor) = visible_input_text(&input, cursor, std::path::Path::new("/"));

    assert_eq!(visible, "open please");
    assert_eq!(mapped_cursor, "open ".len());

    let _ = std::fs::remove_file(file);
}

#[test]
fn attachment_line_count_deduplicates_pending_and_inline_attachments() {
    let first = unique_temp_file("attachment-first");
    let second = unique_temp_file("attachment-second");
    let mut state = TuiState::new(
        TuiAppConfig::default(),
        Size {
            width: 80,
            height: 20,
        },
    );
    state.pending_image_paths = vec![first.display().to_string()];
    state.input = format!(
        "compare @{first} and @{second}",
        first = first.display(),
        second = second.display()
    );

    assert_eq!(attachment_line_count(&state, 80), 2);

    let _ = std::fs::remove_file(first);
    let _ = std::fs::remove_file(second);
}

#[test]
fn measure_input_tracks_cursor_after_wrapping() {
    let wrapped = measure_input("hello", 5, 4);

    assert_eq!(wrapped.lines, vec!["hell".to_string(), "o".to_string()]);
    assert_eq!(wrapped.cursor_row, 1);
    assert_eq!(wrapped.cursor_col, 1);
}

#[test]
fn input_monitor_renders_below_input_border() {
    let mut state = TuiState::new(
        TuiAppConfig::default(),
        Size {
            width: 48,
            height: 12,
        },
    );
    state.input = "hello".to_string();
    state.cursor = state.input.len();
    state.input_monitor = Some("cache hit 80.0% • effective 66.7% • R12k".to_string());

    let wrap = measure_input(&state.input, state.cursor, 48);
    let (lines, cursor) = render_input(&state, 2, 48, 4, wrap);
    let plain = lines
        .iter()
        .map(|line| crate::utils::strip_ansi(line))
        .collect::<Vec<_>>();

    assert!(plain[1].contains("hello"));
    assert!(plain[2].contains("─"));
    assert!(plain[3].contains("cache hit 80.0%"));
    assert!(cursor.is_some());
}

#[test]
fn current_layout_reserves_extra_row_for_input_monitor_when_space_allows() {
    let mut state = TuiState::new(
        TuiAppConfig::default(),
        Size {
            width: 60,
            height: 20,
        },
    );
    state.input = "hello".to_string();
    let base_height = state.current_layout().input.height;

    state.input_monitor = Some("cache hit 50.0%".to_string());
    let monitored_height = state.current_layout().input.height;

    assert_eq!(monitored_height, base_height + 1);
}

#[test]
fn auth_dialog_scrolls_to_keep_input_visible() {
    let mut state = TuiState::new(
        TuiAppConfig::default(),
        Size {
            width: 48,
            height: 8,
        },
    );
    state.input = "verification-code".to_string();
    state.auth_dialog = Some(TuiAuthDialog {
        title: "Sign in".to_string(),
        status: Some("Waiting for browser approval".to_string()),
        steps: vec![],
        url: Some("https://example.com/authorize".to_string()),
        lines: vec![
            "line 1".to_string(),
            "line 2".to_string(),
            "line 3".to_string(),
            "line 4".to_string(),
            "line 5".to_string(),
        ],
        input_label: Some("Enter code".to_string()),
        input_placeholder: Some("Paste here...".to_string()),
    });

    let (lines, cursor) = render_auth_dialog(&state, 48, 8).expect("dialog should render");
    let joined = lines
        .iter()
        .map(|(_, line)| crate::utils::strip_ansi(line))
        .collect::<Vec<_>>()
        .join("\n");

    assert!(joined.contains("Enter code"));
    assert!(joined.contains("verification-code"));
    assert!(!joined.contains("line 1"));
    assert!(cursor.is_some());
}

#[test]
fn approval_input_renders_deny_cursor_inside_visible_panel() {
    let mut state = TuiState::new(
        TuiAppConfig::default(),
        Size {
            width: 60,
            height: 10,
        },
    );
    state.approval_dialog = Some(TuiApprovalDialog {
        title: "Approve command".to_string(),
        command: "rm -rf /tmp/demo".to_string(),
        reason: "Needs permission".to_string(),
        lines: vec!["dangerous command".to_string()],
        allow_session: true,
        session_scope_label: Some("rm".to_string()),
        deny_input: "use a safer directory".to_string(),
        deny_cursor: 0,
        deny_input_placeholder: None,
        selected: TuiApprovalChoice::Deny,
    });

    let (lines, cursor) = render_approval_input(&state, 3, 60, 8);
    let joined = lines
        .iter()
        .map(|line| crate::utils::strip_ansi(line))
        .collect::<Vec<_>>()
        .join("\n");

    assert!(joined.contains("steer: use a safer directory"));
    assert!(cursor.is_some());
}
