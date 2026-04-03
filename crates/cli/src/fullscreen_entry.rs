use std::collections::VecDeque;

use anyhow::Result;
use bb_core::agent_session::PromptOptions;
use bb_core::agent_session_runtime::AgentSessionRuntimeHost;
use bb_core::types::{AgentMessage, ContentBlock, EntryBase, EntryId, SessionEntry, UserMessage};
use bb_session::store;
use bb_tui::footer::detect_git_branch;
use bb_tui::fullscreen::{
    FullscreenAppConfig, FullscreenCommand, FullscreenFooterData, FullscreenNoteLevel, Transcript,
};
use chrono::Utc;
use serde_json::Value;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::interactive::{
    InteractiveEntryOptions, InteractiveModeOptions, InteractiveSessionSetup,
    prepare_interactive_mode,
};
use crate::turn_runner::{self, TurnConfig, TurnEvent};

pub async fn run_fullscreen_entry(entry: InteractiveEntryOptions) -> Result<()> {
    let (runtime_host, options, session_setup) = prepare_interactive_mode(entry).await?;
    let config = build_fullscreen_config(&session_setup);
    let (command_tx, command_rx) = mpsc::unbounded_channel();
    let (submission_tx, submission_rx) = mpsc::unbounded_channel();
    let controller_command_tx = command_tx.clone();

    let controller = FullscreenController::new(runtime_host, options, session_setup, command_tx);
    let controller_task = async move {
        let result = controller.run(submission_rx).await;
        if let Err(err) = &result {
            let _ = controller_command_tx.send(FullscreenCommand::PushNote {
                level: FullscreenNoteLevel::Error,
                text: err.to_string(),
            });
        }
        result
    };

    let (ui_result, controller_result) = tokio::join!(
        bb_tui::fullscreen::run_with_channels(config, command_rx, submission_tx),
        controller_task,
    );

    ui_result?;
    controller_result?;
    Ok(())
}

fn build_fullscreen_config(session_setup: &InteractiveSessionSetup) -> FullscreenAppConfig {
    let transcript = Transcript::new();

    FullscreenAppConfig {
        title: format!("BB-Agent v{}", env!("CARGO_PKG_VERSION")),
        input_placeholder: "Type a prompt for BB-Agent…".to_string(),
        status_line:
            "Esc quits • Ctrl+O transcript • Enter submits • Shift+Enter inserts a newline • wheel/click transcript"
                .to_string(),
        footer: build_footer_data(session_setup),
        transcript,
    }
}

fn build_footer_data(session_setup: &InteractiveSessionSetup) -> FullscreenFooterData {
    let cwd_display = shorten_home_path(&session_setup.tool_ctx.cwd.display().to_string());
    let line1 = if let Some(branch) = detect_git_branch(&session_setup.tool_ctx.cwd.display().to_string()) {
        format!("{cwd_display} ({branch})")
    } else {
        cwd_display
    };

    let line2_left = format!(
        "?/{ctx} (auto)",
        ctx = format_tokens(session_setup.model.context_window)
    );
    let line2_right = format!(
        "({}) {}{}",
        session_setup.model.provider,
        session_setup.model.id,
        if session_setup.thinking_level == "off" {
            " • thinking off".to_string()
        } else {
            format!(" • {}", session_setup.thinking_level)
        }
    );

    FullscreenFooterData {
        line1,
        line2_left,
        line2_right,
    }
}

fn shorten_home_path(path: &str) -> String {
    if let Ok(home) = std::env::var("HOME") {
        if path.starts_with(&home) {
            return format!("~{}", &path[home.len()..]);
        }
    }
    path.to_string()
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

struct FullscreenController {
    runtime_host: AgentSessionRuntimeHost,
    session_setup: InteractiveSessionSetup,
    options: InteractiveModeOptions,
    command_tx: mpsc::UnboundedSender<FullscreenCommand>,
    abort_token: CancellationToken,
    streaming: bool,
    queued_prompts: VecDeque<String>,
    shutdown_requested: bool,
}

impl FullscreenController {
    fn new(
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
            queued_prompts: VecDeque::new(),
            shutdown_requested: false,
        }
    }

    async fn run(mut self, mut submission_rx: mpsc::UnboundedReceiver<String>) -> Result<()> {
        self.publish_footer();

        let startup_prompts = self.options.initial_messages.len()
            + usize::from(self.options.initial_message.is_some());
        if startup_prompts > 0 {
            self.send_command(FullscreenCommand::PushNote {
                level: FullscreenNoteLevel::Status,
                text: format!("Submitting {startup_prompts} startup prompt(s)..."),
            });
            self.publish_status();
        }

        if let Some(initial_message) = self.options.initial_message.clone() {
            self.handle_submitted_text(initial_message, &mut submission_rx)
                .await?;
        }

        for message in self.options.initial_messages.clone() {
            self.handle_submitted_text(message, &mut submission_rx).await?;
        }

        while !self.shutdown_requested {
            let Some(text) = submission_rx.recv().await else {
                self.abort_token.cancel();
                break;
            };
            self.handle_submitted_text(text, &mut submission_rx).await?;
        }

        Ok(())
    }

    async fn handle_submitted_text(
        &mut self,
        text: String,
        submission_rx: &mut mpsc::UnboundedReceiver<String>,
    ) -> Result<()> {
        let text = text.trim().to_string();
        if text.is_empty() || text == "/" {
            return Ok(());
        }

        if text == "/quit" || text == "/exit" {
            self.shutdown_requested = true;
            self.abort_token.cancel();
            return Ok(());
        }

        if self.streaming {
            self.queued_prompts.push_back(text.clone());
            self.send_command(FullscreenCommand::PushNote {
                level: FullscreenNoteLevel::Status,
                text: format!("Queued follow-up: {text}"),
            });
            self.publish_status();
            return Ok(());
        }

        self.dispatch_prompt(text, submission_rx).await?;
        self.drain_queued_prompts(submission_rx).await
    }

    async fn dispatch_prompt(
        &mut self,
        prompt: String,
        submission_rx: &mut mpsc::UnboundedReceiver<String>,
    ) -> Result<()> {
        self.runtime_host
            .session_mut()
            .prompt(prompt.clone(), PromptOptions::default())
            .map_err(anyhow::Error::new)?;

        if self.session_setup.api_key.trim().is_empty() {
            self.send_command(FullscreenCommand::PushNote {
                level: FullscreenNoteLevel::Error,
                text: format!(
                    "No API key configured for provider '{}'. Configure credentials and try again.",
                    self.session_setup.model.provider
                ),
            });
            self.publish_status();
            return Ok(());
        }

        self.ensure_session_row_created()?;
        self.append_user_entry_to_db(&prompt)?;
        self.auto_name_session(&prompt);
        self.publish_footer();
        self.publish_status();
        self.run_streaming_turn_loop(submission_rx).await
    }

    async fn drain_queued_prompts(
        &mut self,
        submission_rx: &mut mpsc::UnboundedReceiver<String>,
    ) -> Result<()> {
        while !self.shutdown_requested {
            let Some(prompt) = self.queued_prompts.pop_front() else {
                break;
            };
            self.publish_status();
            self.dispatch_prompt(prompt, submission_rx).await?;
        }
        Ok(())
    }

    fn send_command(&mut self, command: FullscreenCommand) {
        if self.command_tx.send(command).is_err() {
            self.shutdown_requested = true;
        }
    }

    fn publish_status(&mut self) {
        self.send_command(FullscreenCommand::SetStatusLine(self.status_line()));
    }

    fn publish_footer(&mut self) {
        self.send_command(FullscreenCommand::SetFooter(self.current_footer_data()));
    }

    fn current_footer_data(&self) -> FullscreenFooterData {
        let cwd = self.session_setup.tool_ctx.cwd.display().to_string();
        let mut line1 = if let Some(branch) = detect_git_branch(&cwd) {
            format!("{} ({branch})", shorten_home_path(&cwd))
        } else {
            shorten_home_path(&cwd)
        };

        if let Ok(Some(row)) = store::get_session(&self.session_setup.conn, &self.session_setup.session_id) {
            if let Some(name) = row.name {
                if !name.is_empty() {
                    line1.push_str(" • ");
                    line1.push_str(&name);
                }
            }
        }

        let (input_tokens, output_tokens, cache_read, cache_write, cost) = self.footer_usage_totals();
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
        left_parts.push(format!("?/{ctx} (auto)", ctx = format_tokens(context_window)));

        let right = if self.session_setup.thinking_level == "off" {
            format!("({}) {} • thinking off", self.session_setup.model.provider, self.session_setup.model.id)
        } else {
            format!("({}) {} • {}", self.session_setup.model.provider, self.session_setup.model.id, self.session_setup.thinking_level)
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

        if let Ok(rows) = store::get_entries(&self.session_setup.conn, &self.session_setup.session_id) {
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

        (total_input, total_output, total_cache_read, total_cache_write, total_cost)
    }

    fn status_line(&self) -> String {
        let mut status = String::from(
            "Esc quits • Ctrl+O transcript • Enter submits • Shift+Enter inserts a newline • wheel scrolls transcript",
        );
        if self.streaming {
            status.push_str(" • streaming");
        }
        if !self.queued_prompts.is_empty() {
            status.push_str(&format!(" • queued {}", self.queued_prompts.len()));
        }
        status
    }

    fn ensure_session_row_created(&mut self) -> Result<()> {
        if self.session_setup.session_created {
            return Ok(());
        }

        let cwd = self.session_setup.tool_ctx.cwd.display().to_string();
        store::create_session_with_id(
            &self.session_setup.conn,
            &self.session_setup.session_id,
            &cwd,
        )?;
        self.session_setup.session_created = true;
        Ok(())
    }

    fn append_user_entry_to_db(&mut self, prompt: &str) -> Result<()> {
        let user_entry = SessionEntry::Message {
            base: EntryBase {
                id: EntryId::generate(),
                parent_id: self.get_session_leaf(),
                timestamp: Utc::now(),
            },
            message: AgentMessage::User(UserMessage {
                content: vec![ContentBlock::Text {
                    text: prompt.to_string(),
                }],
                timestamp: Utc::now().timestamp_millis(),
            }),
        };

        store::append_entry(
            &self.session_setup.conn,
            &self.session_setup.session_id,
            &user_entry,
        )?;
        Ok(())
    }

    fn auto_name_session(&mut self, prompt: &str) {
        let session_row =
            store::get_session(&self.session_setup.conn, &self.session_setup.session_id)
                .ok()
                .flatten();
        if session_row
            .as_ref()
            .and_then(|row| row.name.as_deref())
            .is_some()
        {
            return;
        }

        let name = prompt.trim().replace('\n', " ");
        let name = if name.chars().count() > 80 {
            let truncated: String = name.chars().take(77).collect();
            format!("{truncated}...")
        } else {
            name
        };

        let _ = store::set_session_name(
            &self.session_setup.conn,
            &self.session_setup.session_id,
            Some(&name),
        );
    }

    fn build_turn_config(&mut self) -> Result<TurnConfig> {
        let sibling_conn = if let Some(conn) = self.session_setup.sibling_conn.clone() {
            conn
        } else {
            let conn = turn_runner::open_sibling_conn(&self.session_setup.conn)?;
            self.session_setup.sibling_conn = Some(conn.clone());
            conn
        };
        let tools = std::mem::take(&mut self.session_setup.tools);

        Ok(TurnConfig {
            conn: sibling_conn,
            session_id: self.session_setup.session_id.clone(),
            system_prompt: self.session_setup.system_prompt.clone(),
            model: self.session_setup.model.clone(),
            provider: self.session_setup.provider.clone(),
            api_key: self.session_setup.api_key.clone(),
            base_url: self.session_setup.base_url.clone(),
            tools,
            tool_defs: self.session_setup.tool_defs.clone(),
            tool_ctx: bb_tools::ToolContext {
                cwd: self.session_setup.tool_ctx.cwd.clone(),
                artifacts_dir: self.session_setup.tool_ctx.artifacts_dir.clone(),
                on_output: None,
            },
            thinking: if self.session_setup.thinking_level == "off" {
                None
            } else {
                Some(self.session_setup.thinking_level.clone())
            },
            retry_enabled: self.session_setup.retry_enabled,
            retry_max_retries: self.session_setup.retry_max_retries,
            retry_base_delay_ms: self.session_setup.retry_base_delay_ms,
            retry_max_delay_ms: self.session_setup.retry_max_delay_ms,
            cancel: self.abort_token.clone(),
        })
    }

    async fn run_streaming_turn_loop(
        &mut self,
        submission_rx: &mut mpsc::UnboundedReceiver<String>,
    ) -> Result<()> {
        self.streaming = true;
        self.abort_token = CancellationToken::new();
        self.publish_status();

        let turn_config = self.build_turn_config()?;
        let (turn_event_tx, mut turn_event_rx) = mpsc::unbounded_channel::<TurnEvent>();
        let turn_handle =
            tokio::spawn(async move { turn_runner::run_turn(turn_config, turn_event_tx).await });

        let mut aborted = false;
        let mut saw_context_overflow = false;

        loop {
            tokio::select! {
                maybe_event = turn_event_rx.recv() => {
                    let Some(event) = maybe_event else {
                        break;
                    };
                    if matches!(&event, TurnEvent::ContextOverflow { .. }) {
                        saw_context_overflow = true;
                    }
                    self.handle_turn_event(&event);
                    if self.shutdown_requested {
                        self.abort_token.cancel();
                        aborted = true;
                        break;
                    }
                    if saw_context_overflow {
                        break;
                    }
                }
                maybe_prompt = submission_rx.recv() => {
                    match maybe_prompt {
                        Some(text) => {
                            let text = text.trim().to_string();
                            if text.is_empty() || text == "/" {
                                continue;
                            }
                            if text == "/quit" || text == "/exit" {
                                self.shutdown_requested = true;
                                self.abort_token.cancel();
                                aborted = true;
                                break;
                            }
                            self.queued_prompts.push_back(text.clone());
                            self.send_command(FullscreenCommand::PushNote {
                                level: FullscreenNoteLevel::Status,
                                text: format!("Queued follow-up: {text}"),
                            });
                            self.publish_status();
                            if self.shutdown_requested {
                                self.abort_token.cancel();
                                aborted = true;
                                break;
                            }
                        }
                        None => {
                            self.abort_token.cancel();
                            aborted = true;
                            break;
                        }
                    }
                }
            }
        }

        let (returned_config, turn_result) =
            match tokio::time::timeout(std::time::Duration::from_secs(5), turn_handle).await {
                Ok(Ok((config, result))) => (Some(config), result),
                Ok(Err(err)) => {
                    self.send_command(FullscreenCommand::PushNote {
                        level: FullscreenNoteLevel::Error,
                        text: format!("Turn runner task failed: {err}"),
                    });
                    (None, Ok(()))
                }
                Err(_) => {
                    self.send_command(FullscreenCommand::PushNote {
                        level: FullscreenNoteLevel::Warning,
                        text: "Timed out waiting for the turn runner to finish".to_string(),
                    });
                    (None, Ok(()))
                }
            };

        if let Some(config) = returned_config {
            self.session_setup.tools = config.tools;
        }

        if saw_context_overflow {
            self.send_command(FullscreenCommand::PushNote {
                level: FullscreenNoteLevel::Warning,
                text: "Context overflow detected. The shared fullscreen path does not auto-compact yet; switch to the legacy interactive mode to recover.".to_string(),
            });
        }

        if let Err(err) = turn_result {
            self.send_command(FullscreenCommand::PushNote {
                level: FullscreenNoteLevel::Error,
                text: err.to_string(),
            });
        }

        if aborted {
            self.send_command(FullscreenCommand::TurnAborted);
        }

        self.streaming = false;
        self.publish_footer();
        self.publish_status();
        Ok(())
    }

    fn handle_turn_event(&mut self, event: &TurnEvent) {
        match event {
            TurnEvent::TurnStart { turn_index } => {
                self.send_command(FullscreenCommand::TurnStart {
                    turn_index: *turn_index,
                });
            }
            TurnEvent::TextDelta(text) => {
                self.send_command(FullscreenCommand::TextDelta(text.clone()));
            }
            TurnEvent::ThinkingDelta(text) => {
                self.send_command(FullscreenCommand::ThinkingDelta(text.clone()));
            }
            TurnEvent::ToolCallStart { id, name } => {
                self.send_command(FullscreenCommand::ToolCallStart {
                    id: id.clone(),
                    name: name.clone(),
                });
            }
            TurnEvent::ToolCallDelta { id, args } => {
                self.send_command(FullscreenCommand::ToolCallDelta {
                    id: id.clone(),
                    args: args.clone(),
                });
            }
            TurnEvent::ToolExecuting { id, .. } => {
                self.send_command(FullscreenCommand::ToolExecuting { id: id.clone() });
            }
            TurnEvent::ToolResult {
                id,
                name,
                content,
                details,
                artifact_path,
                is_error,
            } => {
                self.send_command(FullscreenCommand::ToolResult {
                    id: id.clone(),
                    name: name.clone(),
                    content: content.clone(),
                    details: details.clone(),
                    artifact_path: artifact_path.clone(),
                    is_error: *is_error,
                });
            }
            TurnEvent::TurnEnd { .. } => {
                self.send_command(FullscreenCommand::TurnEnd);
            }
            TurnEvent::ContextOverflow { message } => {
                self.send_command(FullscreenCommand::PushNote {
                    level: FullscreenNoteLevel::Warning,
                    text: message.clone(),
                });
            }
            TurnEvent::AutoRetryStart {
                attempt,
                max_attempts,
                delay_ms,
                error_message,
            } => {
                self.send_command(FullscreenCommand::PushNote {
                    level: FullscreenNoteLevel::Status,
                    text: format!(
                        "Retrying ({attempt}/{max_attempts}) in {}s: {error_message}",
                        ((delay_ms + 500) / 1000).max(1)
                    ),
                });
            }
            TurnEvent::AutoRetryEnd {
                success,
                attempt,
                final_error,
            } => {
                self.send_command(FullscreenCommand::PushNote {
                    level: if *success {
                        FullscreenNoteLevel::Status
                    } else {
                        FullscreenNoteLevel::Warning
                    },
                    text: if *success {
                        format!("Retry {attempt} succeeded")
                    } else {
                        format!(
                            "Retry {attempt} failed: {}",
                            final_error
                                .clone()
                                .unwrap_or_else(|| "unknown error".to_string())
                        )
                    },
                });
            }
            TurnEvent::Done { .. } => {}
            TurnEvent::Error(message) => {
                self.send_command(FullscreenCommand::TurnError {
                    message: message.clone(),
                });
                self.send_command(FullscreenCommand::PushNote {
                    level: FullscreenNoteLevel::Error,
                    text: message.clone(),
                });
            }
        }
    }

    fn get_session_leaf(&self) -> Option<EntryId> {
        turn_runner::get_leaf_raw(&self.session_setup.conn, &self.session_setup.session_id)
    }
}

#[allow(dead_code)]
fn format_tool_result_content(
    content: &[ContentBlock],
    details: Option<&Value>,
    artifact_path: Option<&str>,
) -> String {
    let mut sections = Vec::new();

    let mut rendered_content = String::new();
    for block in content {
        match block {
            ContentBlock::Text { text } => {
                if !rendered_content.is_empty() {
                    rendered_content.push('\n');
                }
                rendered_content.push_str(text);
            }
            ContentBlock::Image { mime_type, .. } => {
                if !rendered_content.is_empty() {
                    rendered_content.push('\n');
                }
                rendered_content.push_str(&format!("[image output: {mime_type}]"));
            }
        }
    }
    if !rendered_content.trim().is_empty() {
        sections.push(rendered_content);
    }

    if let Some(details) = details {
        let details = serde_json::to_string_pretty(details).unwrap_or_else(|_| details.to_string());
        sections.push(format!("details:\n{details}"));
    }

    if let Some(path) = artifact_path {
        sections.push(format!("artifact: {path}"));
    }

    if sections.is_empty() {
        "(no textual output)".to_string()
    } else {
        sections.join("\n\n")
    }
}
