use std::collections::VecDeque;

use anyhow::Result;
use bb_core::agent_session_runtime::AgentSessionRuntimeHost;
use bb_session::store;
use bb_tui::footer::detect_git_branch;
use bb_tui::fullscreen::{
    FullscreenCommand, FullscreenFooterData, FullscreenSubmission,
};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::interactive::{InteractiveModeOptions, InteractiveSessionSetup};
use crate::slash::dispatch_local_slash_command;

use super::{format_tokens, shorten_home_path};

pub(super) struct FullscreenController {
    pub(super) runtime_host: AgentSessionRuntimeHost,
    pub(super) session_setup: InteractiveSessionSetup,
    pub(super) options: InteractiveModeOptions,
    pub(super) command_tx: mpsc::UnboundedSender<FullscreenCommand>,
    pub(super) abort_token: CancellationToken,
    pub(super) streaming: bool,
    pub(super) retry_status: Option<String>,
    pub(super) queued_prompts: VecDeque<String>,
    pub(super) shutdown_requested: bool,
}

impl FullscreenController {
    pub(super) fn new(
        runtime_host: AgentSessionRuntimeHost,
        options: InteractiveModeOptions,
        session_setup: InteractiveSessionSetup,
        command_tx: mpsc::UnboundedSender<FullscreenCommand>,
    ) -> Self {
        Self {
            runtime_host,
            session_setup,
            options,
            command_tx,
            abort_token: CancellationToken::new(),
            streaming: false,
            retry_status: None,
            queued_prompts: VecDeque::new(),
            shutdown_requested: false,
        }
    }

    pub(super) async fn run(
        mut self,
        mut submission_rx: mpsc::UnboundedReceiver<FullscreenSubmission>,
    ) -> Result<()> {
        self.publish_footer();

        if let Some(initial_message) = self.options.initial_message.take() {
            self.handle_submitted_text(initial_message, &mut submission_rx)
                .await?;
        }

        for message in std::mem::take(&mut self.options.initial_messages) {
            self.handle_submitted_text(message, &mut submission_rx)
                .await?;
        }

        while !self.shutdown_requested {
            let Some(submission) = submission_rx.recv().await else {
                self.abort_token.cancel();
                break;
            };
            self.handle_submission(submission, &mut submission_rx)
                .await?;
        }

        Ok(())
    }

    async fn handle_submission(
        &mut self,
        submission: FullscreenSubmission,
        submission_rx: &mut mpsc::UnboundedReceiver<FullscreenSubmission>,
    ) -> Result<()> {
        match submission {
            FullscreenSubmission::Input(text) => {
                self.handle_submitted_text(text, submission_rx).await
            }
            FullscreenSubmission::MenuSelection { menu_id, value } => {
                self.handle_menu_selection(&menu_id, &value).await
            }
        }
    }

    pub(super) async fn handle_submitted_text(
        &mut self,
        text: String,
        submission_rx: &mut mpsc::UnboundedReceiver<FullscreenSubmission>,
    ) -> Result<()> {
        let text = text.trim().to_string();
        if text.is_empty() || text == "/" {
            return Ok(());
        }

        if self.handle_local_submission(&text).await? {
            return Ok(());
        }

        if self.streaming {
            self.queued_prompts.push_back(text);
            self.publish_status();
            return Ok(());
        }

        self.dispatch_prompt(text, submission_rx).await?;
        self.drain_queued_prompts(submission_rx).await
    }

    pub(super) async fn handle_local_submission(&mut self, text: &str) -> Result<bool> {
        dispatch_local_slash_command(self, text)
    }

    pub(super) fn send_command(&mut self, command: FullscreenCommand) {
        if self.command_tx.send(command).is_err() {
            self.shutdown_requested = true;
        }
    }

    pub(super) fn publish_status(&mut self) {
        self.send_command(FullscreenCommand::SetStatusLine(self.status_line()));
    }

    pub(super) fn publish_footer(&mut self) {
        self.send_command(FullscreenCommand::SetFooter(self.current_footer_data()));
    }

    fn status_line(&self) -> String {
        if let Some(status) = &self.retry_status {
            return status.to_string();
        }

        let mut status = if self.streaming {
            String::from("Working...")
        } else {
            String::new()
        };
        if !self.queued_prompts.is_empty() {
            if status.is_empty() {
                status = format!("Queued {}", self.queued_prompts.len());
            } else {
                status.push_str(&format!(" • queued {}", self.queued_prompts.len()));
            }
        }
        status
    }

    fn current_footer_data(&self) -> FullscreenFooterData {
        let cwd = self.session_setup.tool_ctx.cwd.display().to_string();
        let mut line1 = if let Some(branch) = detect_git_branch(&cwd) {
            format!("{} ({branch})", shorten_home_path(&cwd))
        } else {
            shorten_home_path(&cwd)
        };

        if let Ok(Some(row)) =
            store::get_session(&self.session_setup.conn, &self.session_setup.session_id)
        {
            if let Some(name) = row.name {
                if !name.is_empty() {
                    line1.push_str(" • ");
                    line1.push_str(&name);
                }
            }
        }

        let (input_tokens, output_tokens, cache_read, cache_write, cost) =
            self.footer_usage_totals();
        let mut left_parts = Vec::new();
        if input_tokens > 0 {
            left_parts.push(format!("↑{}", format_tokens(input_tokens)));
        }
        if output_tokens > 0 {
            left_parts.push(format!("↓{}", format_tokens(output_tokens)));
        }
        if cache_read > 0 {
            left_parts.push(format!("R{}", format_tokens(cache_read)));
        }
        if cache_write > 0 {
            left_parts.push(format!("W{}", format_tokens(cache_write)));
        }
        if cost > 0.0 {
            left_parts.push(format!("${cost:.3}"));
        }
        let context_window = self.session_setup.model.context_window;
        left_parts.push(format!(
            "?/{ctx} (auto)",
            ctx = format_tokens(context_window)
        ));

        let right = if self.session_setup.thinking_level == "off" {
            format!(
                "({}) {} • thinking off",
                self.session_setup.model.provider, self.session_setup.model.id
            )
        } else {
            format!(
                "({}) {} • {}",
                self.session_setup.model.provider,
                self.session_setup.model.id,
                self.session_setup.thinking_level
            )
        };

        FullscreenFooterData {
            line1,
            line2_left: left_parts.join(" "),
            line2_right: right,
        }
    }

    fn footer_usage_totals(&self) -> (u64, u64, u64, u64, f64) {
        let mut total_input = 0_u64;
        let mut total_output = 0_u64;
        let mut total_cache_read = 0_u64;
        let mut total_cache_write = 0_u64;
        let mut total_cost = 0.0_f64;

        if let Ok(rows) =
            store::get_entries(&self.session_setup.conn, &self.session_setup.session_id)
        {
            for row in rows {
                if let Ok(entry) = store::parse_entry(&row) {
                    if let bb_core::types::SessionEntry::Message {
                        message: bb_core::types::AgentMessage::Assistant(message),
                        ..
                    } = entry
                    {
                        total_input += message.usage.input;
                        total_output += message.usage.output;
                        total_cache_read += message.usage.cache_read;
                        total_cache_write += message.usage.cache_write;
                        total_cost += message.usage.cost.total;
                    }
                }
            }
        }

        (
            total_input,
            total_output,
            total_cache_read,
            total_cache_write,
            total_cost,
        )
    }
}
