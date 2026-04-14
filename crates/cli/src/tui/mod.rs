mod controller;
mod formatting;
mod menus;
mod session;
mod turns;

use std::io::Write;

use anyhow::Result;
use bb_tools::{ToolApprovalOutcome, ToolApprovalRequest};
use bb_tui::footer::detect_git_branch;
use bb_tui::tui::{Transcript, TuiAppConfig, TuiCommand, TuiFooterData, TuiNoteLevel};
use tokio::sync::mpsc;

use crate::session_bootstrap::{
    SessionBootstrapOptions, SessionRuntimeSetup, prepare_session_runtime,
};
use crate::session_info::permission_posture_badge;

use controller::{PendingApprovalRequest, TuiController};

const LOGIN_PROVIDER_MENU_ID: &str = "login-provider";
const LOGIN_METHOD_MENU_ID: &str = "login-method";
const LOGOUT_PROVIDER_MENU_ID: &str = "logout-provider";
const RESUME_SESSION_MENU_ID: &str = "resume-session";
const TREE_ENTRY_MENU_ID: &str = "tree-entry";
const TREE_SUMMARY_MENU_ID: &str = "tree-summary";
const FORK_ENTRY_MENU_ID: &str = "fork-entry";
const LOGIN_PROVIDERS: &[&str] = &[
    "anthropic",
    "openai",
    "github-copilot",
    "google",
    "groq",
    "xai",
    "openrouter",
];

pub(crate) async fn run_tui_entry(entry: SessionBootstrapOptions) -> Result<()> {
    let (runtime_host, options, mut session_setup) = prepare_session_runtime(entry).await?;
    let extra_slash_items = build_dynamic_slash_items(&runtime_host);
    let config = build_tui_config(&session_setup, &options.prompt_label, extra_slash_items);
    let (command_tx, command_rx) = mpsc::unbounded_channel();
    let (submission_tx, submission_rx) = mpsc::unbounded_channel();
    let (approval_tx, approval_rx) = mpsc::unbounded_channel();
    let controller_command_tx = command_tx.clone();

    session_setup.tool_ctx.request_approval =
        Some(std::sync::Arc::new(move |request: ToolApprovalRequest| {
            let approval_tx = approval_tx.clone();
            Box::pin(async move {
                let (response_tx, response_rx) = tokio::sync::oneshot::channel();
                if approval_tx
                    .send(PendingApprovalRequest {
                        request,
                        response_tx,
                    })
                    .is_err()
                {
                    return ToolApprovalOutcome {
                        decision: bb_tools::ToolApprovalDecision::Denied,
                    };
                }
                response_rx.await.unwrap_or(ToolApprovalOutcome {
                    decision: bb_tools::ToolApprovalDecision::Denied,
                })
            })
        }));

    let controller = TuiController::new(
        runtime_host,
        options,
        session_setup,
        command_tx,
        approval_rx,
    );
    let controller_task = async move {
        let result = controller.run(submission_rx).await;
        if let Err(err) = &result {
            let _ = controller_command_tx.send(TuiCommand::PushNote {
                level: TuiNoteLevel::Error,
                text: err.to_string(),
            });
        }
        result
    };

    let (ui_result, controller_result) = tokio::join!(
        bb_tui::tui::run_with_channels(config, command_rx, submission_tx),
        controller_task,
    );

    ui_result?;
    controller_result?;
    Ok(())
}

pub(super) fn build_dynamic_slash_items(
    runtime_host: &bb_core::agent_session_runtime::AgentSessionRuntimeHost,
) -> Vec<bb_tui::select_list::SelectItem> {
    let mut items = Vec::new();

    // Skills
    for skill in &runtime_host.bootstrap().resource_bootstrap.skills {
        items.push(bb_tui::select_list::SelectItem {
            label: format!("/skill:{}", skill.info.name),
            detail: None,
            value: format!("/skill:{}", skill.info.name),
        });
    }

    // Prompt templates
    for prompt in &runtime_host.bootstrap().resource_bootstrap.prompts {
        items.push(bb_tui::select_list::SelectItem {
            label: format!("/{}", prompt.info.name),
            detail: Some(prompt.info.description.clone()),
            value: format!("/{}", prompt.info.name),
        });
    }

    // Extension commands
    for cmd in &runtime_host
        .bootstrap()
        .resource_bootstrap
        .extensions
        .registered_commands
    {
        items.push(bb_tui::select_list::SelectItem {
            label: format!("/{}", cmd.invocation_name),
            detail: Some(cmd.description.clone()),
            value: format!("/{}", cmd.invocation_name),
        });
    }

    items
}

fn build_tui_config(
    session_setup: &SessionRuntimeSetup,
    prompt_label: &str,
    extra_slash_items: Vec<bb_tui::select_list::SelectItem>,
) -> TuiAppConfig {
    let transcript = Transcript::new();

    let title = if prompt_label.is_empty() || prompt_label == "default" {
        format!("♡ BB-Agent v{}", env!("CARGO_PKG_VERSION"))
    } else {
        format!(
            "♡ BB-Agent v{} • prompt: {}",
            env!("CARGO_PKG_VERSION"),
            prompt_label
        )
    };

    TuiAppConfig {
        title,
        input_placeholder: "Type a prompt for BB-Agent…".to_string(),
        status_line: String::new(),
        footer: build_footer_data(session_setup),
        transcript,
        extra_slash_items,
        cwd: session_setup.tool_ctx.cwd.clone(),
    }
}

fn build_footer_data(session_setup: &SessionRuntimeSetup) -> TuiFooterData {
    let cwd_display = shorten_home_path(&session_setup.tool_ctx.cwd.display().to_string());
    let line1 = if let Some(branch) =
        detect_git_branch(&session_setup.tool_ctx.cwd.display().to_string())
    {
        format!("{cwd_display} ({branch})")
    } else {
        cwd_display
    };

    let line2_left = format!(
        "?/{ctx} (auto)",
        ctx = format_tokens(session_setup.model.context_window)
    );
    let line2_right = format!(
        "({}) {}{} • {}",
        session_setup.model.provider,
        session_setup.model.id,
        if session_setup.thinking_level == "off" {
            " • thinking off".to_string()
        } else {
            format!(" • {}", session_setup.thinking_level)
        },
        permission_posture_badge(session_setup.tool_ctx.execution_policy)
    );

    TuiFooterData {
        line1,
        line2_left,
        line2_right,
    }
}

fn shorten_home_path(path: &str) -> String {
    if let Ok(home) = std::env::var("HOME")
        && path.starts_with(&home)
    {
        return format!("~{}", &path[home.len()..]);
    }
    path.to_string()
}

/// Shorten a path for display (home prefix + long paths).
pub(super) fn shorten_path(path: &str) -> String {
    shorten_home_path(path)
}

fn format_tokens(count: u64) -> String {
    if count < 1_000 {
        count.to_string()
    } else if count < 10_000 {
        format!("{:.1}k", count as f64 / 1_000.0)
    } else if count < 1_000_000 {
        format!("{}k", (count as f64 / 1_000.0).round() as u64)
    } else if count < 10_000_000 {
        format!("{:.1}M", count as f64 / 1_000_000.0)
    } else {
        format!("{}M", (count as f64 / 1_000_000.0).round() as u64)
    }
}

fn base64_encode_simple(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    let mut i = 0;
    while i < data.len() {
        let b0 = data[i] as u32;
        let b1 = if i + 1 < data.len() {
            data[i + 1] as u32
        } else {
            0
        };
        let b2 = if i + 2 < data.len() {
            data[i + 2] as u32
        } else {
            0
        };
        let n = (b0 << 16) | (b1 << 8) | b2;
        let c0 = ((n >> 18) & 0x3F) as usize;
        let c1 = ((n >> 12) & 0x3F) as usize;
        let c2 = ((n >> 6) & 0x3F) as usize;
        let c3 = (n & 0x3F) as usize;
        out.push(TABLE[c0] as char);
        out.push(TABLE[c1] as char);
        if i + 1 < data.len() {
            out.push(TABLE[c2] as char);
        } else {
            out.push('=');
        }
        if i + 2 < data.len() {
            out.push(TABLE[c3] as char);
        } else {
            out.push('=');
        }
        i += 3;
    }
    out
}

fn copy_text_to_clipboard(text: &str) -> Result<()> {
    let encoded = base64_encode_simple(text.as_bytes());
    let mut stdout = std::io::stdout();
    write!(stdout, "\x1b]52;c;{encoded}\x07")?;
    stdout.flush()?;
    Ok(())
}
