use std::collections::VecDeque;
use std::path::Path;

use bb_session::store;
use bb_tui::footer::detect_git_branch;
use bb_tui::tui::{TuiCommand, TuiFooterData};

use crate::session_info::{collect_session_info_summary, permission_posture_badge};

use super::{TuiController, PendingImage, QueuedPrompt};
use crate::tui::{format_tokens, shorten_home_path};

fn cleanup_managed_clipboard_temp_image(path: &Path) {
    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
        return;
    };
    if path.parent() == Some(std::env::temp_dir().as_path())
        && file_name.starts_with("bb-clipboard-")
        && file_name.ends_with(".png")
    {
        let _ = std::fs::remove_file(path);
    }
}

impl TuiController {
    /// Read image files from disk and queue them as pending images for the next prompt.
    pub(crate) fn attach_images_from_paths(&mut self, paths: &[String]) {
        use base64::Engine;

        for path in paths {
            let resolved = if std::path::Path::new(path).is_absolute() {
                std::path::PathBuf::from(path)
            } else {
                self.session_setup.tool_ctx.cwd.join(path)
            };
            let data = match std::fs::read(&resolved) {
                Ok(data) => data,
                Err(error) => {
                    tracing::warn!("Cannot read image {path}: {error}");
                    cleanup_managed_clipboard_temp_image(&resolved);
                    continue;
                }
            };
            let Some(mime_type) = image_mime_type(&resolved) else {
                cleanup_managed_clipboard_temp_image(&resolved);
                continue;
            };
            let encoded = base64::engine::general_purpose::STANDARD.encode(&data);
            self.pending_images.push(PendingImage {
                data: encoded,
                mime_type: mime_type.to_string(),
            });
            cleanup_managed_clipboard_temp_image(&resolved);
        }
    }

    pub(crate) fn send_command(&mut self, command: TuiCommand) {
        if self.command_tx.send(command).is_err() {
            self.shutdown_requested = true;
        }
    }

    pub(crate) fn publish_status(&mut self) {
        self.send_command(TuiCommand::SetStatusLine(self.status_line()));
    }

    pub(crate) fn show_startup_resources(&mut self) {
        let bootstrap = &self.runtime_host.bootstrap().resource_bootstrap;
        tracing::debug!(
            "show_startup_resources: skills={} prompts={} extensions={}",
            bootstrap.skills.len(),
            bootstrap.prompts.len(),
            bootstrap.extensions.extensions.len()
        );

        let sections = [
            resource_section(
                "Skills",
                bootstrap.skills.iter().flat_map(|skill| {
                    let path = super::super::shorten_path(&skill.info.source_info.path);
                    [
                        format!("  /skill:{}", skill.info.name),
                        format!("    {path}"),
                    ]
                }),
            ),
            resource_section(
                "Prompts",
                bootstrap.prompts.iter().flat_map(|prompt| {
                    let path = super::super::shorten_path(&prompt.info.source_info.path);
                    [format!("  /{}", prompt.info.name), format!("    {path}")]
                }),
            ),
            resource_section(
                "Extensions",
                bootstrap
                    .extensions
                    .extensions
                    .iter()
                    .map(|extension| super::super::shorten_path(&extension.path)),
            ),
        ]
        .into_iter()
        .flatten()
        .flatten()
        .collect::<Vec<_>>();

        if sections.is_empty() {
            return;
        }

        self.send_command(TuiCommand::PushNote {
            level: bb_tui::tui::TuiNoteLevel::Status,
            text: sections.join("\n"),
        });
    }

    pub(crate) fn publish_footer(&mut self) {
        self.send_command(TuiCommand::SetFooter(self.current_footer_data()));
    }

    pub(crate) fn mark_local_settings_saved(&mut self) {
        self.resource_watch = super::ResourceWatchState::capture(&self.session_setup.tool_ctx.cwd);
        self.suppress_next_resource_watch_reload = true;
    }

    fn status_line(&self) -> String {
        build_status_line(
            self.retry_status.as_deref(),
            self.manual_compaction_in_progress,
            self.auto_compaction_in_progress,
            &self.queued_prompts,
        )
    }

    fn current_footer_data(&self) -> TuiFooterData {
        let cwd = self.session_setup.tool_ctx.cwd.display().to_string();
        let mut line1 = footer_line1(
            &cwd,
            &self.session_setup.conn,
            &self.session_setup.session_id,
        );
        if !self.queued_prompts.is_empty() {
            let queue_hint = format!(
                "↳ Alt+Up to edit all queued messages{}",
                if self.manual_compaction_in_progress || self.auto_compaction_in_progress {
                    format!(" • queued {}", self.queued_prompts.len())
                } else {
                    String::new()
                }
            );
            line1 = if line1.is_empty() {
                queue_hint
            } else {
                format!("{line1} • {queue_hint}")
            };
        }

        let (input_tokens, output_tokens, cache_read, cache_write, cost) =
            self.footer_usage_totals();
        let mut left_parts = Vec::new();
        push_usage_part(&mut left_parts, input_tokens, "↑");
        push_usage_part(&mut left_parts, output_tokens, "↓");
        push_usage_part(&mut left_parts, cache_read, "R");
        push_usage_part(&mut left_parts, cache_write, "W");
        if cost > 0.0 {
            left_parts.push(format!("${cost:.3}"));
        }
        left_parts.push(self.current_context_footer_text());

        let right = if self.session_setup.thinking_level == "off" {
            format!(
                "({}) {} • thinking off • {}",
                self.session_setup.model.provider,
                self.session_setup.model.id,
                permission_posture_badge(self.session_setup.tool_ctx.execution_policy)
            )
        } else {
            format!(
                "({}) {} • {} • {}",
                self.session_setup.model.provider,
                self.session_setup.model.id,
                self.session_setup.thinking_level,
                permission_posture_badge(self.session_setup.tool_ctx.execution_policy)
            )
        };

        TuiFooterData {
            line1,
            line2_left: left_parts.join(" "),
            line2_right: right,
        }
    }

    fn current_context_footer_text(&self) -> String {
        let active_path =
            bb_session::tree::active_path(&self.session_setup.conn, &self.session_setup.session_id)
                .ok();
        let latest_entry_is_compaction = active_path
            .as_ref()
            .and_then(|rows| rows.last())
            .is_some_and(|row| row.entry_type == "compaction");
        let runtime_usage = if self.manual_compaction_in_progress
            || self.auto_compaction_in_progress
            || latest_entry_is_compaction
        {
            None
        } else {
            self.runtime_host.runtime().get_context_usage()
        };
        let context_window = runtime_usage
            .as_ref()
            .map(|usage| usage.context_window as u64)
            .filter(|window| *window > 0)
            .unwrap_or(self.session_setup.model.context_window);
        let compaction_enabled =
            bb_core::settings::Settings::load_merged(&self.session_setup.tool_ctx.cwd)
                .compaction
                .enabled;
        let auto_suffix = compaction_auto_suffix(compaction_enabled);

        if let Some(usage) = runtime_usage {
            if let Some(tokens) = usage.tokens {
                return format_context_from_tokens(tokens as u64, context_window, auto_suffix);
            }
            if let Some(percent) = usage.percent {
                return format_context_percent(percent as f64, context_window, auto_suffix);
            }
            return format_unknown_context(context_window, auto_suffix);
        }

        if self.manual_compaction_in_progress
            || self.auto_compaction_in_progress
            || latest_entry_is_compaction
        {
            return format_unknown_context(context_window, auto_suffix);
        }

        if let Some(tokens) = active_path
            .as_deref()
            .and_then(estimate_active_path_context_tokens)
        {
            return format_context_from_tokens(tokens, context_window, auto_suffix);
        }

        format_unknown_context(context_window, auto_suffix)
    }

    fn footer_usage_totals(&self) -> (u64, u64, u64, u64, f64) {
        collect_session_info_summary(
            &self.session_setup.conn,
            &self.session_setup.session_id,
            &self.session_setup.model.provider,
            &self.session_setup.model.id,
            &self.session_setup.thinking_level,
            self.session_setup.tool_ctx.execution_policy,
        )
        .map(|summary| {
            (
                summary.input_tokens,
                summary.output_tokens,
                summary.cache_read_tokens,
                summary.cache_write_tokens,
                summary.total_cost,
            )
        })
        .unwrap_or((0, 0, 0, 0, 0.0))
    }
}

fn build_status_line(
    retry_status: Option<&str>,
    manual_compaction_in_progress: bool,
    auto_compaction_in_progress: bool,
    queued_prompts: &VecDeque<QueuedPrompt>,
) -> String {
    if let Some(status) = retry_status {
        return status.to_string();
    }
    if manual_compaction_in_progress {
        return compaction_status_line("Compacting session...", queued_prompts);
    }
    if auto_compaction_in_progress {
        return compaction_status_line("Auto-compacting session...", queued_prompts);
    }
    queued_prompt_status_line(queued_prompts).unwrap_or_default()
}

fn compaction_status_line(label: &str, queued_prompts: &VecDeque<QueuedPrompt>) -> String {
    if queued_prompts.is_empty() {
        label.to_string()
    } else {
        format!("{label} • {} queued", queued_prompts.len())
    }
}

fn queued_prompt_status_line(queued_prompts: &VecDeque<QueuedPrompt>) -> Option<String> {
    let last = queued_prompts.back()?;
    let last = match last {
        QueuedPrompt::Visible(text) | QueuedPrompt::Hidden(text) => text,
    };
    let preview = last.replace('\n', " ⏎ ");
    let preview: String = preview.chars().take(80).collect();
    Some(format!("Steering: {preview}"))
}

fn compaction_auto_suffix(enabled: bool) -> &'static str {
    if enabled { " (auto)" } else { "" }
}

fn image_mime_type(path: &std::path::Path) -> Option<&'static str> {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_lowercase())
        .as_deref()
    {
        Some("png") => Some("image/png"),
        Some("jpg" | "jpeg") => Some("image/jpeg"),
        Some("gif") => Some("image/gif"),
        Some("webp") => Some("image/webp"),
        _ => None,
    }
}

fn resource_section<I>(title: &str, lines: I) -> Option<Vec<String>>
where
    I: IntoIterator<Item = String>,
{
    let body = lines.into_iter().collect::<Vec<_>>();
    if body.is_empty() {
        return None;
    }
    let mut section = Vec::with_capacity(body.len() + 1);
    section.push(format!("[{title}]"));
    section.extend(body);
    Some(section)
}

fn footer_line1(cwd: &str, conn: &rusqlite::Connection, session_id: &str) -> String {
    let mut line1 = if let Some(branch) = detect_git_branch(cwd) {
        format!("{} ({branch})", shorten_home_path(cwd))
    } else {
        shorten_home_path(cwd)
    };

    if let Ok(Some(row)) = store::get_session(conn, session_id)
        && let Some(name) = row.name
        && !name.is_empty()
    {
        line1.push_str(" • ");
        line1.push_str(&name);
    }

    line1
}

fn push_usage_part(parts: &mut Vec<String>, tokens: u64, prefix: &str) {
    if tokens > 0 {
        parts.push(format!("{prefix}{}", format_tokens(tokens)));
    }
}

fn format_context_percent(percent: f64, context_window: u64, auto_suffix: &str) -> String {
    format!(
        "{percent:.1}%/{}{}",
        format_tokens(context_window),
        auto_suffix
    )
}

fn format_context_from_tokens(tokens: u64, context_window: u64, auto_suffix: &str) -> String {
    if context_window == 0 {
        return format_unknown_context(context_window, auto_suffix);
    }
    format_context_percent(
        (tokens as f64 / context_window as f64) * 100.0,
        context_window,
        auto_suffix,
    )
}

fn format_unknown_context(context_window: u64, auto_suffix: &str) -> String {
    format!("?/{}{}", format_tokens(context_window), auto_suffix)
}

// Keep footer context reporting aligned with compaction/runtime estimation.
// In particular, do not reuse stale pre-compaction assistant usage after the
// latest compaction boundary; treat context usage as unknown until a fresh
// post-compaction assistant usage record exists.
fn estimate_active_path_context_tokens(path: &[bb_session::store::EntryRow]) -> Option<u64> {
    let latest_compaction_index = path.iter().rposition(|row| row.entry_type == "compaction");
    if let Some(compaction_index) = latest_compaction_index {
        let has_post_compaction_usage = path.iter().skip(compaction_index + 1).rev().any(|row| {
            let Ok(entry) = bb_session::store::parse_entry(row) else {
                return false;
            };
            match entry {
                bb_core::types::SessionEntry::Message {
                    message: bb_core::types::AgentMessage::Assistant(assistant),
                    ..
                } => {
                    assistant.stop_reason != bb_core::types::StopReason::Aborted
                        && assistant.stop_reason != bb_core::types::StopReason::Error
                        && bb_session::compaction::calculate_context_tokens(&assistant.usage) > 0
                }
                _ => false,
            }
        });
        if !has_post_compaction_usage {
            return None;
        }
    }

    bb_session::context::build_context_from_path(path)
        .ok()
        .map(|ctx| bb_session::compaction::estimate_context_tokens(&ctx.messages).tokens)
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::{
        QueuedPrompt, build_status_line, compaction_auto_suffix,
        estimate_active_path_context_tokens, format_context_from_tokens, format_context_percent,
        format_unknown_context, permission_posture_badge,
    };
    use bb_core::types::{
        AgentMessage, AssistantContent, AssistantMessage, EntryBase, EntryId, SessionEntry,
        StopReason, Usage,
    };
    use bb_session::{store, tree};
    use bb_tools::ExecutionPolicy;
    use chrono::Utc;

    #[test]
    fn permission_badge_is_compact_for_footer_use() {
        assert_eq!(
            permission_posture_badge(ExecutionPolicy::Safety),
            "mode safety/project-only"
        );
        assert_eq!(
            permission_posture_badge(ExecutionPolicy::Yolo),
            "mode yolo/full-access"
        );
    }

    #[test]
    fn compaction_auto_suffix_reflects_real_enabled_state() {
        assert_eq!(compaction_auto_suffix(true), " (auto)");
        assert_eq!(compaction_auto_suffix(false), "");
    }

    #[test]
    fn status_line_prioritizes_manual_compaction_over_queued_steering() {
        let mut queued = VecDeque::new();
        queued.push_back(QueuedPrompt::Visible("run tests after compact".to_string()));

        assert_eq!(
            build_status_line(None, true, false, &queued),
            "Compacting session... • 1 queued"
        );
    }

    #[test]
    fn status_line_shows_auto_compaction_state_before_steering_preview() {
        let mut queued = VecDeque::new();
        queued.push_back(QueuedPrompt::Visible("first".to_string()));
        queued.push_back(QueuedPrompt::Hidden("second".to_string()));

        assert_eq!(
            build_status_line(None, false, true, &queued),
            "Auto-compacting session... • 2 queued"
        );
    }

    #[test]
    fn active_path_context_estimate_uses_usage_aware_estimator() {
        let conn = store::open_memory().expect("memory db");
        let session_id = store::create_session(&conn, "/tmp").expect("session");
        let assistant = SessionEntry::Message {
            base: EntryBase {
                id: EntryId::generate(),
                parent_id: None,
                timestamp: Utc::now(),
            },
            message: AgentMessage::Assistant(AssistantMessage {
                content: vec![AssistantContent::Text {
                    text: "tiny text".to_string(),
                }],
                provider: "openai".to_string(),
                model: "gpt-5.4".to_string(),
                usage: Usage {
                    total_tokens: 120_000,
                    ..Default::default()
                },
                stop_reason: StopReason::Stop,
                error_message: None,
                timestamp: Utc::now().timestamp_millis(),
            }),
        };
        store::append_entry(&conn, &session_id, &assistant).expect("append assistant");

        let path = tree::active_path(&conn, &session_id).expect("active path");
        let estimated = estimate_active_path_context_tokens(&path).expect("estimated tokens");

        assert_eq!(estimated, 120_000);
    }

    #[test]
    fn context_footer_formatters_preserve_zero_percent() {
        assert_eq!(
            format_context_from_tokens(0, 272_000, " (auto)"),
            "0.0%/272k (auto)"
        );
        assert_eq!(format_context_percent(0.0, 272_000, ""), "0.0%/272k");
        assert_eq!(format_unknown_context(272_000, " (auto)"), "?/272k (auto)");
    }

    #[test]
    fn active_path_context_estimate_ignores_stale_usage_before_latest_compaction() {
        let conn = store::open_memory().expect("memory db");
        let session_id = store::create_session(&conn, "/tmp").expect("session");

        let assistant = SessionEntry::Message {
            base: EntryBase {
                id: EntryId::generate(),
                parent_id: None,
                timestamp: Utc::now(),
            },
            message: AgentMessage::Assistant(AssistantMessage {
                content: vec![AssistantContent::Text {
                    text: "before compact".to_string(),
                }],
                provider: "openai".to_string(),
                model: "gpt-5.4".to_string(),
                usage: Usage {
                    total_tokens: 240_000,
                    ..Default::default()
                },
                stop_reason: StopReason::Stop,
                error_message: None,
                timestamp: Utc::now().timestamp_millis(),
            }),
        };
        store::append_entry(&conn, &session_id, &assistant).expect("append assistant");

        let compaction = SessionEntry::Compaction {
            base: EntryBase {
                id: EntryId::generate(),
                parent_id: Some(assistant.base().id.clone()),
                timestamp: Utc::now(),
            },
            summary: "summary".to_string(),
            first_kept_entry_id: assistant.base().id.clone(),
            tokens_before: 240_000,
            details: None,
            from_plugin: false,
        };
        store::append_entry(&conn, &session_id, &compaction).expect("append compaction");

        let user = SessionEntry::Message {
            base: EntryBase {
                id: EntryId::generate(),
                parent_id: Some(compaction.base().id.clone()),
                timestamp: Utc::now(),
            },
            message: AgentMessage::User(bb_core::types::UserMessage {
                content: vec![bb_core::types::ContentBlock::Text {
                    text: "12345678".to_string(),
                }],
                timestamp: Utc::now().timestamp_millis(),
            }),
        };
        store::append_entry(&conn, &session_id, &user).expect("append user");

        let path = tree::active_path(&conn, &session_id).expect("active path");
        let estimated = estimate_active_path_context_tokens(&path);

        assert!(
            estimated.is_none(),
            "expected post-compaction context usage to stay unknown until fresh assistant usage"
        );
    }
}
