use super::*;
use crate::fullscreen::FullscreenAppConfig;
use crate::fullscreen::format_tool_result_content;
use crate::fullscreen::layout::Size;
use crate::fullscreen::runtime::FullscreenState;
use crate::fullscreen::types::FullscreenCommand;
use bb_core::types::ContentBlock;

#[test]
fn active_turn_status_uses_elapsed_progress_instead_of_static_status_line() {
    let mut state = FullscreenState::new(
        FullscreenAppConfig::default(),
        Size {
            width: 80,
            height: 20,
        },
    );
    let _ = state.apply_command(FullscreenCommand::TurnStart { turn_index: 0 });
    state.status_line = "Working...".to_string();

    let first = render_status(&state, 80);
    let plain_first = crate::utils::strip_ansi(&first);
    assert!(plain_first.contains("requesting response •"));
    assert!(!plain_first.contains("Working..."));
    assert!(first.contains("\x1b[38;2;")); // truecolor escapes

    for _ in 0..3 {
        state.on_tick();
    }
    let later = render_status(&state, 80);
    let plain_later = crate::utils::strip_ansi(&later);
    assert!(plain_later.contains("requesting response •"));
}

#[test]
fn local_action_status_uses_animated_spinner_with_elapsed_time() {
    let mut state = FullscreenState::new(
        FullscreenAppConfig::default(),
        Size {
            width: 80,
            height: 20,
        },
    );
    let _ = state.apply_command(FullscreenCommand::SetLocalActionActive(true));
    let _ = state.apply_command(FullscreenCommand::SetStatusLine(
        "Compacting session... (Esc to cancel)".to_string(),
    ));

    let first = render_status(&state, 80);
    let plain_first = crate::utils::strip_ansi(&first);
    assert!(plain_first.contains("Compacting session... (Esc to cancel) • "));
    assert!(first.contains("\x1b[38;2;"));

    for _ in 0..3 {
        state.on_tick();
    }
    let later = render_status(&state, 80);
    let plain_later = crate::utils::strip_ansi(&later);
    assert!(plain_later.contains("Compacting session... (Esc to cancel) • "));
}

#[test]
fn transcript_mode_active_turn_uses_spinner_status() {
    let mut state = FullscreenState::new(
        FullscreenAppConfig::default(),
        Size {
            width: 80,
            height: 20,
        },
    );
    let _ = state.apply_command(FullscreenCommand::TurnStart { turn_index: 0 });
    state.mode = crate::fullscreen::types::FullscreenMode::Transcript;

    let rendered = render_status(&state, 80);
    let plain = crate::utils::strip_ansi(&rendered);
    assert!(plain.contains("requesting response •"));
    assert!(rendered.contains("\x1b[38;2;"));
}

#[test]
fn thinking_and_writing_status_render_with_elapsed_progress() {
    let mut state = FullscreenState::new(
        FullscreenAppConfig::default(),
        Size {
            width: 80,
            height: 20,
        },
    );
    let _ = state.apply_command(FullscreenCommand::TurnStart { turn_index: 0 });
    let _ = state.apply_command(FullscreenCommand::ThinkingDelta("pondering".to_string()));
    state.on_tick();

    let thinking = render_status(&state, 80);
    let thinking_plain = crate::utils::strip_ansi(&thinking);
    assert!(thinking_plain.contains("thinking •"));

    let _ = state.apply_command(FullscreenCommand::TextDelta("answer".to_string()));
    state.on_tick();

    let writing = render_status(&state, 80);
    let writing_plain = crate::utils::strip_ansi(&writing);
    assert!(writing_plain.contains("writing •"));
}

#[test]
fn plain_status_line_is_visually_separated_from_transcript_content() {
    let state = FullscreenState::new(
        FullscreenAppConfig::default(),
        Size {
            width: 80,
            height: 20,
        },
    );
    let rendered = render_status(&state, 80);
    let plain = crate::utils::strip_ansi(&rendered);
    assert!(plain.contains("Ctrl+Shift+O expands tools"));

    let mut state = FullscreenState::new(
        FullscreenAppConfig::default(),
        Size {
            width: 80,
            height: 20,
        },
    );
    state.status_line = "Resumed session".to_string();
    let rendered = render_status(&state, 80);
    let plain = crate::utils::strip_ansi(&rendered);
    assert!(plain.contains("· Resumed session"));
}

#[test]
fn tool_use_rows_stay_aligned_when_focused_in_transcript_mode() {
    let mut config = FullscreenAppConfig::default();
    let mut transcript = crate::fullscreen::transcript::Transcript::new();
    let assistant = transcript.append_root_block(
        crate::fullscreen::transcript::NewBlock::new(
            crate::fullscreen::transcript::BlockKind::AssistantMessage,
            "assistant",
        )
        .with_content("done"),
    );
    let tool = transcript
        .append_child_block(
            assistant,
            crate::fullscreen::transcript::NewBlock::new(
                crate::fullscreen::transcript::BlockKind::ToolUse,
                "bash",
            )
            .with_content("echo hi"),
        )
        .expect("tool block");
    config.transcript = transcript;

    let mut state = FullscreenState::new(
        config,
        Size {
            width: 80,
            height: 20,
        },
    );
    state.mode = crate::fullscreen::types::FullscreenMode::Transcript;
    state.focused_block = Some(tool);

    let lines = render_transcript(&state, &state.projection, 80, 20);
    let tool_line = lines
        .into_iter()
        .find(|line| crate::utils::strip_ansi(line).contains("▶ Bash"))
        .expect("tool line should render");

    let plain = crate::utils::strip_ansi(&tool_line);
    let t = crate::theme::theme();
    assert!(plain.contains("▶ Bash"));
    assert!(tool_line.contains("\x1b[7m"));
    assert!(tool_line.contains(&t.tool_pending_bg));
}

#[test]
fn transcript_blank_line_rhythm_matches_user_tool_text_flow() {
    let mut transcript = crate::fullscreen::transcript::Transcript::new();
    transcript.append_root_block(
        crate::fullscreen::transcript::NewBlock::new(
            crate::fullscreen::transcript::BlockKind::UserMessage,
            "prompt",
        )
        .with_content("check my disk usage"),
    );
    let assistant_tools = transcript.append_root_block(
        crate::fullscreen::transcript::NewBlock::new(
            crate::fullscreen::transcript::BlockKind::AssistantMessage,
            "assistant",
        )
        .with_content(""),
    );
    let tool = transcript
        .append_child_block(
            assistant_tools,
            crate::fullscreen::transcript::NewBlock::new(
                crate::fullscreen::transcript::BlockKind::ToolUse,
                "Bash(df -h) • done",
            )
            .with_expandable(true),
        )
        .expect("tool block");
    let _ = transcript
        .append_child_block(
            tool,
            crate::fullscreen::transcript::NewBlock::new(
                crate::fullscreen::transcript::BlockKind::ToolResult,
                "output",
            )
            .with_content(format!(
                "Ran 1 command ({})",
                crate::ui_hints::TOOL_EXPAND_HINT
            )),
        )
        .expect("tool result");
    transcript.append_root_block(
        crate::fullscreen::transcript::NewBlock::new(
            crate::fullscreen::transcript::BlockKind::AssistantMessage,
            "assistant",
        )
        .with_content("summary text"),
    );

    let state = FullscreenState::new(
        FullscreenAppConfig {
            transcript,
            ..FullscreenAppConfig::default()
        },
        Size {
            width: 100,
            height: 24,
        },
    );

    let lines = render_transcript(&state, &state.projection, 100, 24)
        .into_iter()
        .map(|line| crate::utils::strip_ansi(&line))
        .collect::<Vec<_>>();
    let user_idx = lines
        .iter()
        .position(|line| line.contains("❯ check my disk usage"))
        .expect("user line");
    let tool_idx = lines
        .iter()
        .position(|line| line.contains("● Bash(df -h)"))
        .expect("tool header");
    let summary_idx = lines
        .iter()
        .position(|line| {
            line.contains(&format!("Ran 1 command ({})", crate::ui_hints::TOOL_EXPAND_HINT))
        })
        .expect("summary line");
    let text_idx = lines
        .iter()
        .position(|line| line.contains("summary text"))
        .expect("assistant text");

    assert!(lines[user_idx + 1].trim().is_empty());
    assert_eq!(tool_idx, user_idx + 2);
    assert_eq!(summary_idx, tool_idx + 1);
    assert!(lines[summary_idx + 1].trim().is_empty());
    assert_eq!(text_idx, summary_idx + 2);
}

#[test]
fn transcript_uses_prompt_tool_and_response_layout_markers() {
    let mut transcript = crate::fullscreen::transcript::Transcript::new();
    transcript.append_root_block(
        crate::fullscreen::transcript::NewBlock::new(
            crate::fullscreen::transcript::BlockKind::UserMessage,
            "prompt",
        )
        .with_content("check my disk usage"),
    );
    let assistant = transcript.append_root_block(
        crate::fullscreen::transcript::NewBlock::new(
            crate::fullscreen::transcript::BlockKind::AssistantMessage,
            "assistant",
        )
        .with_content("The disk usage looks healthy."),
    );
    let tool = transcript
        .append_child_block(
            assistant,
            crate::fullscreen::transcript::NewBlock::new(
                crate::fullscreen::transcript::BlockKind::ToolUse,
                "Bash(df -h / /home 2>/dev/null | sort -u) • done",
            )
            .with_expandable(true),
        )
        .expect("tool block");
    let _ = transcript
            .append_child_block(
                tool,
                crate::fullscreen::transcript::NewBlock::new(
                    crate::fullscreen::transcript::BlockKind::ToolResult,
                    "output",
                )
                .with_content("/dev/nvme0n1p2  1.8T  931G  808G  54% /\nFilesystem      Size  Used Avail Use% Mounted on"),
            )
            .expect("tool result");

    let state = FullscreenState::new(
        FullscreenAppConfig {
            transcript,
            ..FullscreenAppConfig::default()
        },
        Size {
            width: 100,
            height: 24,
        },
    );

    let lines = render_transcript(&state, &state.projection, 100, 24)
        .into_iter()
        .map(|line| crate::utils::strip_ansi(&line))
        .collect::<Vec<_>>();

    assert!(
        lines
            .iter()
            .any(|line| line.contains("❯ check my disk usage"))
    );
    assert!(
        lines
            .iter()
            .any(|line| line.contains("● Bash(df -h / /home 2>/dev/null | sort -u)"))
    );
    assert!(
        lines
            .iter()
            .any(|line| line.contains("  ⎿  /dev/nvme0n1p2"))
    );
    assert!(lines.iter().any(|line| line.contains("     Filesystem")));
}

#[test]
fn syntax_highlighted_read_result_does_not_leak_raw_ansi_parameters() {
    let rendered = format_tool_result_content(
        "read",
        &[ContentBlock::Text {
            text: "        let _ = event_tx.send(turn_event);\n        });".to_string(),
        }],
        Some(serde_json::json!({
            "path": "/tmp/demo.rs",
            "startLine": 380,
            "endLine": 381,
            "totalLines": 434
        })),
        None,
        false,
        true,
    );

    let mut transcript = crate::fullscreen::transcript::Transcript::new();
    let assistant = transcript.append_root_block(
        crate::fullscreen::transcript::NewBlock::new(
            crate::fullscreen::transcript::BlockKind::AssistantMessage,
            "assistant",
        )
        .with_content("done"),
    );
    let tool = transcript
        .append_child_block(
            assistant,
            crate::fullscreen::transcript::NewBlock::new(
                crate::fullscreen::transcript::BlockKind::ToolUse,
                "Read(/tmp/demo.rs:380-381) • done",
            )
            .with_expandable(true),
        )
        .expect("tool block");
    let _ = transcript
        .append_child_block(
            tool,
            crate::fullscreen::transcript::NewBlock::new(
                crate::fullscreen::transcript::BlockKind::ToolResult,
                "output",
            )
            .with_content(rendered),
        )
        .expect("tool result");

    let state = FullscreenState::new(
        FullscreenAppConfig {
            transcript,
            ..FullscreenAppConfig::default()
        },
        Size {
            width: 28,
            height: 16,
        },
    );

    let lines = render_transcript(&state, &state.projection, 28, 16);
    for line in &lines {
        let plain = crate::utils::strip_ansi(line);
        assert!(
            !plain.contains("38;2;") && !plain.contains("[38;2;"),
            "raw ANSI parameters leaked into transcript row: {plain:?}"
        );
    }
}

#[test]
fn pending_tool_header_stays_stable_until_result_arrives() {
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
        args: serde_json::json!({"command":"echo hi"}).to_string(),
    });
    let _ = state.apply_command(FullscreenCommand::ToolExecuting {
        id: "tool-1".into(),
    });
    state.prepare_for_render();

    let first = render_transcript(&state, &state.projection, 80, 24)
        .into_iter()
        .map(|line| crate::utils::strip_ansi(&line))
        .find(|line| line.contains("Bash"))
        .expect("first pending header");

    for _ in 0..7 {
        state.on_tick();
    }
    state.prepare_for_render();
    let second = render_transcript(&state, &state.projection, 80, 24)
        .into_iter()
        .map(|line| crate::utils::strip_ansi(&line))
        .find(|line| line.contains("Bash"))
        .expect("second pending header");

    assert!(first.contains("Bash(echo hi)"));
    assert!(second.contains("Bash(echo hi)"));
    assert_eq!(
        first.chars().skip(1).collect::<String>(),
        second.chars().skip(1).collect::<String>()
    );
}

#[test]
fn pending_tool_header_has_no_intermediate_output_placeholder() {
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
        args: serde_json::json!({"command":"echo hi"}).to_string(),
    });
    let _ = state.apply_command(FullscreenCommand::ToolExecuting {
        id: "tool-1".into(),
    });
    state.prepare_for_render();

    let lines = render_transcript(&state, &state.projection, 80, 24);
    let header = lines
        .iter()
        .find(|line| crate::utils::strip_ansi(line).contains("Bash"))
        .expect("tool header");

    let plain_header = crate::utils::strip_ansi(header);

    assert!(plain_header.contains("Bash(echo hi)"));
    assert!(plain_header.starts_with('●') || plain_header.starts_with('·'));
    assert!(!lines.iter().any(|line| line.contains("executing...")));
}

#[test]
fn summary_notes_render_with_header_block() {
    let mut transcript = crate::fullscreen::transcript::Transcript::new();
    transcript.append_root_block(
        crate::fullscreen::transcript::NewBlock::new(
            crate::fullscreen::transcript::BlockKind::SystemNote,
            "branch summary",
        )
        .with_content("summarized work here"),
    );

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
    let lines = render_transcript(&state, &state.projection, 80, 16)
        .into_iter()
        .map(|line| crate::utils::strip_ansi(&line))
        .collect::<Vec<_>>();

    assert!(lines.iter().any(|line| line.contains("Branch Summary")));
    assert!(
        lines
            .iter()
            .any(|line| line.contains("summarized work here"))
    );
}

#[test]
fn expanded_compaction_block_keeps_highlight_background_on_blank_lines() {
    let t = crate::theme::theme();
    let mut transcript = crate::fullscreen::transcript::Transcript::new();
    transcript.append_root_block(
        crate::fullscreen::transcript::NewBlock::new(
            crate::fullscreen::transcript::BlockKind::SystemNote,
            "compaction",
        )
        .with_content("[compaction: 123 tokens summarized]\n\nfirst line\n\nthird line")
        .with_expandable(true),
    );

    let state = FullscreenState::new(
        FullscreenAppConfig {
            transcript,
            ..FullscreenAppConfig::default()
        },
        Size {
            width: 50,
            height: 16,
        },
    );
    let lines = render_transcript(&state, &state.projection, 50, 16);

    let header = lines
        .iter()
        .find(|line| crate::utils::strip_ansi(line).contains("[Compact Context]"))
        .expect("compaction header");
    let first = lines
        .iter()
        .find(|line| crate::utils::strip_ansi(line).contains("first line"))
        .expect("first compaction line");
    let third = lines
        .iter()
        .find(|line| crate::utils::strip_ansi(line).contains("third line"))
        .expect("third compaction line");
    let blank_compaction = lines
        .iter()
        .find(|line| line.contains(&t.info_bg) && crate::utils::strip_ansi(line).trim().is_empty())
        .expect("blank compaction line with background");

    assert!(header.contains(&t.info_bg));
    assert!(first.contains(&t.info_bg));
    assert!(third.contains(&t.info_bg));
    assert!(blank_compaction.contains(&t.info_bg));
    assert_eq!(crate::utils::visible_width(blank_compaction), 50);
}

#[test]
fn edit_diff_rows_only_highlight_changed_lines_and_fill_width() {
    let t = crate::theme::theme();
    let diff = format!(
        "applied 1/1 edit(s) to foo.txt\n    {}  1 before{}\n{}    {}- 2 old{}\n{}    {}+ 2 new{}\n    {}  3 after{}",
        t.diff_context,
        t.reset,
        t.diff_removed_bg,
        t.diff_removed,
        t.reset,
        t.diff_added_bg,
        t.diff_added,
        t.reset,
        t.diff_context,
        t.reset,
    );

    let mut transcript = crate::fullscreen::transcript::Transcript::new();
    let assistant = transcript.append_root_block(
        crate::fullscreen::transcript::NewBlock::new(
            crate::fullscreen::transcript::BlockKind::AssistantMessage,
            "assistant",
        )
        .with_content(""),
    );
    let tool = transcript
        .append_child_block(
            assistant,
            crate::fullscreen::transcript::NewBlock::new(
                crate::fullscreen::transcript::BlockKind::ToolUse,
                "Edit(foo.txt) • done",
            )
            .with_expandable(true),
        )
        .expect("tool block");
    let _ = transcript
        .append_child_block(
            tool,
            crate::fullscreen::transcript::NewBlock::new(
                crate::fullscreen::transcript::BlockKind::ToolResult,
                "output",
            )
            .with_content(diff),
        )
        .expect("tool result");

    let width = 60usize;
    let state = FullscreenState::new(
        FullscreenAppConfig {
            transcript,
            ..FullscreenAppConfig::default()
        },
        Size {
            width: width as u16,
            height: 24,
        },
    );
    let lines = render_transcript(&state, &state.projection, width, 24);

    let context = lines
        .iter()
        .find(|line| crate::utils::strip_ansi(line).contains("1 before"))
        .expect("context line");
    let removed = lines
        .iter()
        .find(|line| crate::utils::strip_ansi(line).contains("old"))
        .expect("removed line");
    let added = lines
        .iter()
        .find(|line| crate::utils::strip_ansi(line).contains("new"))
        .expect("added line");

    assert!(!context.contains(&t.diff_removed_bg));
    assert!(!context.contains(&t.diff_added_bg));
    assert!(removed.contains(&t.diff_removed_bg));
    assert!(added.contains(&t.diff_added_bg));

    assert_eq!(crate::utils::visible_width(removed), width);
    assert_eq!(crate::utils::visible_width(added), width);
}
