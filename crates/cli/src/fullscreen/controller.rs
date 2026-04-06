use std::{collections::VecDeque, time::SystemTime};

use bb_core::agent_session_runtime::AgentSessionRuntimeHost;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::session_bootstrap::{SessionRuntimeSetup, SessionUiOptions};

mod loop_impl;
mod resources;
mod ui;

/// An image file queued for attachment to the next prompt.
#[derive(Clone, Debug)]
pub(super) struct PendingImage {
    pub data: String,
    pub mime_type: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct ResourceWatchState {
    global_settings_mtime: Option<SystemTime>,
    project_settings_mtime: Option<SystemTime>,
}

impl ResourceWatchState {
    fn capture(cwd: &std::path::Path) -> Self {
        Self {
            global_settings_mtime: settings_mtime(
                &bb_core::config::global_dir().join("settings.json"),
            ),
            project_settings_mtime: settings_mtime(
                &bb_core::config::project_dir(cwd).join("settings.json"),
            ),
        }
    }
}

fn settings_mtime(path: &std::path::Path) -> Option<SystemTime> {
    std::fs::metadata(path).ok()?.modified().ok()
}

pub(super) struct FullscreenController {
    pub(super) runtime_host: AgentSessionRuntimeHost,
    pub(super) session_setup: SessionRuntimeSetup,
    pub(super) options: SessionUiOptions,
    pub(super) command_tx: mpsc::UnboundedSender<bb_tui::fullscreen::FullscreenCommand>,
    pub(super) abort_token: CancellationToken,
    pub(super) streaming: bool,
    pub(super) retry_status: Option<String>,
    pub(super) queued_prompts: VecDeque<String>,
    pub(super) pending_tree_summary_target: Option<String>,
    pub(super) pending_tree_custom_prompt_target: Option<String>,
    pub(super) pending_login_api_key_provider: Option<String>,
    pub(super) pending_images: Vec<PendingImage>,
    pub(super) local_action_cancel: Option<CancellationToken>,
    pub(super) color_theme: bb_tui::fullscreen::spinner::ColorTheme,
    pub(super) shutdown_requested: bool,
    resource_watch: ResourceWatchState,
    suppress_next_resource_watch_reload: bool,
}

impl FullscreenController {
    pub(super) fn new(
        runtime_host: AgentSessionRuntimeHost,
        options: SessionUiOptions,
        session_setup: SessionRuntimeSetup,
        command_tx: mpsc::UnboundedSender<bb_tui::fullscreen::FullscreenCommand>,
    ) -> Self {
        let resource_watch = ResourceWatchState::capture(&session_setup.tool_ctx.cwd);
        Self {
            runtime_host,
            session_setup,
            options,
            command_tx,
            abort_token: CancellationToken::new(),
            streaming: false,
            retry_status: None,
            queued_prompts: VecDeque::new(),
            pending_tree_summary_target: None,
            pending_tree_custom_prompt_target: None,
            pending_login_api_key_provider: None,
            pending_images: Vec::new(),
            local_action_cancel: None,
            color_theme: bb_tui::fullscreen::spinner::ColorTheme::default(),
            shutdown_requested: false,
            resource_watch,
            suppress_next_resource_watch_reload: false,
        }
    }
}
