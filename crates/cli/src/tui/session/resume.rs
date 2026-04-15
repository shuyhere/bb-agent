use anyhow::Result;
use bb_core::agent_session::ModelRef;
use bb_core::agent_session_runtime::RuntimeModelRef;
use bb_core::settings::Settings;
use bb_provider::Provider;
use bb_provider::anthropic::AnthropicProvider;
use bb_provider::google::GoogleProvider;
use bb_provider::openai::OpenAiProvider;
use bb_provider::registry::{ApiType, ModelRegistry};
use bb_session::{context, store};
use bb_tui::select_list::SelectItem;
use bb_tui::tui::{Transcript, TuiCommand, TuiNoteLevel};

use super::super::RESUME_SESSION_MENU_ID;
use super::super::controller::TuiController;

impl TuiController {
    pub(in crate::tui) fn handle_new_session(&mut self) {
        let new_id = uuid::Uuid::new_v4().to_string();
        self.options.session_id = Some(new_id.clone());
        self.session_setup.session_id = new_id;
        self.session_setup.session_created = false;
        let _ = self.runtime_host.session_mut().clear_queue();
        self.queued_prompts.clear();
        self.pending_tree_summary_target = None;
        self.pending_tree_custom_prompt_target = None;
        self.pending_images.clear();
        self.retry_status = None;
        self.manual_compaction_in_progress = false;
        self.manual_compaction_generation += 1;
        if let Some(cancel) = self.local_action_cancel.take() {
            cancel.cancel();
        }
        self.send_command(TuiCommand::SetLocalActionActive(false));
        self.send_command(TuiCommand::SetTranscript(Transcript::new()));
        self.send_command(TuiCommand::SetInput(String::new()));
        self.publish_footer();
        self.send_command(TuiCommand::PushNote {
            level: TuiNoteLevel::Status,
            text: "New session started".to_string(),
        });
    }

    pub(in crate::tui) fn open_resume_menu(&mut self) -> Result<()> {
        let cwd = self.session_setup.tool_ctx.cwd.display().to_string();
        let sessions = store::list_sessions(&self.session_setup.conn, &cwd)?;
        if sessions.is_empty() {
            self.send_command(TuiCommand::SetStatusLine(
                "No sessions found in this directory.".to_string(),
            ));
            return Ok(());
        }
        let items = sessions
            .into_iter()
            .map(|row| SelectItem {
                label: row
                    .name
                    .clone()
                    .unwrap_or_else(|| row.session_id.chars().take(8).collect()),
                detail: Some(format!("{} entries • {}", row.entry_count, row.updated_at)),
                value: row.session_id,
            })
            .collect();
        self.send_command(TuiCommand::OpenSelectMenu {
            menu_id: RESUME_SESSION_MENU_ID.to_string(),
            title: "Resume session".to_string(),
            items,
            selected_value: None,
        });
        Ok(())
    }

    pub(in crate::tui) async fn handle_resume_session(&mut self, session_id: &str) -> Result<()> {
        self.session_setup.session_id = session_id.to_string();
        self.session_setup.session_created = true;
        self.options.session_id = Some(session_id.to_string());
        let _ = self.runtime_host.session_mut().clear_queue();
        // Clear stale state from previous session's tree interactions.
        self.pending_tree_summary_target = None;
        self.pending_tree_custom_prompt_target = None;
        self.pending_images.clear();
        self.queued_prompts.clear();
        self.streaming = false;
        self.retry_status = None;
        self.manual_compaction_in_progress = false;
        self.manual_compaction_generation += 1;
        if let Some(cancel) = self.local_action_cancel.take() {
            cancel.cancel();
        }
        self.send_command(TuiCommand::SetLocalActionActive(false));
        self.send_command(TuiCommand::SetStatusLine("Resuming session...".to_string()));
        self.send_command(TuiCommand::SetLocalActionActive(true));
        tokio::task::yield_now().await;

        let result: Result<()> = (|| {
            let settings = Settings::load_merged(&self.session_setup.tool_ctx.cwd);
            if let Ok(session_context) =
                context::build_context(&self.session_setup.conn, session_id)
            {
                if let Some(model_info) = session_context.model.clone() {
                    let mut registry = ModelRegistry::new();
                    registry.load_custom_models(&settings);
                    crate::login::add_cached_github_copilot_models(&mut registry);
                    if let Some(model) = registry
                        .find(&model_info.provider, &model_info.model_id)
                        .cloned()
                        .or_else(|| {
                            registry
                                .find_fuzzy(&model_info.model_id, Some(&model_info.provider))
                                .cloned()
                        })
                        .or_else(|| registry.find_fuzzy(&model_info.model_id, None).cloned())
                    {
                        let api_key =
                            crate::login::resolve_api_key(&model.provider).unwrap_or_default();
                        let base_url = if model.provider == "github-copilot" {
                            crate::login::github_copilot_api_base_url()
                        } else {
                            model
                                .base_url
                                .clone()
                                .unwrap_or_else(|| "https://api.openai.com/v1".into())
                        };
                        let headers = if model.provider == "github-copilot" {
                            crate::login::github_copilot_runtime_headers()
                        } else {
                            std::collections::HashMap::new()
                        };
                        let provider: std::sync::Arc<dyn Provider> = match model.api {
                            ApiType::AnthropicMessages => {
                                std::sync::Arc::new(AnthropicProvider::new())
                            }
                            ApiType::GoogleGenerative => std::sync::Arc::new(GoogleProvider::new()),
                            _ => std::sync::Arc::new(OpenAiProvider::new()),
                        };

                        self.runtime_host.session_mut().set_model(ModelRef {
                            provider: model.provider.clone(),
                            id: model.id.clone(),
                            reasoning: model.reasoning,
                        });
                        self.runtime_host
                            .runtime_mut()
                            .set_model(Some(RuntimeModelRef {
                                provider: model.provider.clone(),
                                id: model.id.clone(),
                                context_window: model.context_window as usize,
                            }));
                        self.session_setup.model = model;
                        self.session_setup.provider = provider;
                        self.session_setup.api_key = api_key;
                        self.session_setup.base_url = base_url;
                        self.session_setup.headers = headers.clone();
                        self.session_setup.tool_ctx.web_search = Some(bb_tools::WebSearchRuntime {
                            provider: self.session_setup.provider.clone(),
                            model: self.session_setup.model.clone(),
                            api_key: self.session_setup.api_key.clone(),
                            base_url: self.session_setup.base_url.clone(),
                            headers,
                            enabled: true,
                        });
                        self.options.model_display = Some(format!(
                            "{}/{}",
                            self.session_setup.model.provider, self.session_setup.model.id
                        ));
                    }
                }
            }

            let thinking_level = crate::session_bootstrap::resolve_thinking_level(
                None,
                context::active_path_explicit_thinking_level(&self.session_setup.conn, session_id)
                    .ok()
                    .flatten(),
                settings.default_thinking.as_deref(),
            );
            self.session_setup.thinking_level = thinking_level.as_str().to_string();
            self.runtime_host
                .session_mut()
                .set_thinking_level(thinking_level);

            self.rebuild_current_transcript()?;
            self.send_command(TuiCommand::SetInput(String::new()));
            self.publish_footer();
            Ok(())
        })();

        self.send_command(TuiCommand::SetLocalActionActive(false));
        match result {
            Ok(()) => {
                self.send_command(TuiCommand::SetStatusLine("Resumed session".to_string()));
                Ok(())
            }
            Err(err) => {
                self.send_command(TuiCommand::SetStatusLine("Resume failed".to_string()));
                Err(err)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, VecDeque};
    use std::sync::Arc;

    use bb_core::agent_session_runtime::{AgentSessionRuntimeBootstrap, AgentSessionRuntimeHost};
    use bb_core::types::{
        AgentMessage, ContentBlock, EntryBase, EntryId, SessionEntry, UserMessage,
    };
    use bb_provider::openai::OpenAiProvider;
    use bb_provider::registry::{ApiType, CostConfig, Model, ModelInput};
    use bb_session::store;
    use bb_tools::{ExecutionPolicy, ToolContext, ToolExecutionMode};
    use bb_tui::tui::TuiCommand;
    use chrono::Utc;
    use tokio::sync::mpsc;
    use tokio_util::sync::CancellationToken;

    use crate::extensions::{ExtensionBootstrap, ExtensionCommandRegistry};
    use crate::session_bootstrap::{SessionRuntimeSetup, SessionUiOptions};
    use crate::tui::RESUME_SESSION_MENU_ID;
    use crate::tui::controller::{PendingImage, QueuedPrompt, TuiController};

    fn test_model() -> Model {
        Model {
            id: "gpt-test".to_string(),
            name: "gpt-test".to_string(),
            provider: "openai".to_string(),
            api: ApiType::OpenaiCompletions,
            context_window: 128_000,
            max_tokens: 16_384,
            reasoning: false,
            input: vec![ModelInput::Text],
            base_url: None,
            cost: CostConfig::default(),
        }
    }

    fn build_test_controller() -> (
        TuiController,
        mpsc::UnboundedReceiver<TuiCommand>,
        tempfile::TempDir,
    ) {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let cwd = tempdir.path().to_path_buf();
        let conn = store::open_memory().expect("memory db");
        let model = test_model();
        let tool_ctx = ToolContext {
            cwd: cwd.clone(),
            artifacts_dir: cwd.join("artifacts"),
            execution_policy: ExecutionPolicy::Safety,
            on_output: None,
            web_search: None,
            execution_mode: ToolExecutionMode::Interactive,
            request_approval: None,
        };
        let runtime_host = AgentSessionRuntimeHost::from_bootstrap(AgentSessionRuntimeBootstrap {
            cwd: Some(cwd),
            ..AgentSessionRuntimeBootstrap::default()
        });
        let options = SessionUiOptions {
            session_id: Some("seed-session".to_string()),
            ..SessionUiOptions::default()
        };
        let session_setup = SessionRuntimeSetup {
            conn,
            session_id: "seed-session".to_string(),
            provider: Arc::new(OpenAiProvider::new()),
            model,
            api_key: String::new(),
            base_url: "https://api.openai.com/v1".to_string(),
            headers: HashMap::new(),
            tool_registry: crate::tool_registry::ToolRegistry::default(),
            tool_selection: crate::tool_registry::ToolSelection::All,
            tool_ctx,
            system_prompt: String::new(),
            base_system_prompt: String::new(),
            thinking_level: "medium".to_string(),
            compaction_enabled: true,
            compaction_reserve_tokens: 8_000,
            compaction_keep_recent_tokens: 16_000,
            retry_enabled: true,
            retry_max_retries: 3,
            retry_base_delay_ms: 100,
            retry_max_delay_ms: 1_000,
            session_created: true,
            request_metrics_tracker: Arc::new(tokio::sync::Mutex::new(
                bb_monitor::RequestMetricsTracker::new(),
            )),
            request_metrics_log_path: None,
            sibling_conn: None,
            extension_commands: ExtensionCommandRegistry::default(),
            extension_bootstrap: ExtensionBootstrap::default(),
        };
        let (command_tx, command_rx) = mpsc::unbounded_channel();
        let (_approval_tx, approval_rx) = mpsc::unbounded_channel();
        let controller = TuiController::new(
            runtime_host,
            options,
            session_setup,
            command_tx,
            approval_rx,
        );
        (controller, command_rx, tempdir)
    }

    fn drain_commands(rx: &mut mpsc::UnboundedReceiver<TuiCommand>) -> Vec<TuiCommand> {
        let mut commands = Vec::new();
        while let Ok(command) = rx.try_recv() {
            commands.push(command);
        }
        commands
    }

    #[test]
    fn handle_new_session_clears_transient_state_and_emits_reset_commands() {
        let (mut controller, mut command_rx, _tempdir) = build_test_controller();
        controller.queued_prompts = VecDeque::from([QueuedPrompt::Visible("queued".to_string())]);
        controller.pending_tree_summary_target = Some("tree-1".to_string());
        controller.pending_tree_custom_prompt_target = Some("tree-2".to_string());
        controller.pending_images.push(PendingImage {
            data: "abcd".to_string(),
            mime_type: "image/png".to_string(),
        });
        controller.retry_status = Some("retrying".to_string());
        controller.manual_compaction_in_progress = true;
        controller.manual_compaction_generation = 7;
        let cancel = CancellationToken::new();
        controller.local_action_cancel = Some(cancel.clone());
        let previous_session_id = controller.session_setup.session_id.clone();

        controller.handle_new_session();

        assert_ne!(controller.session_setup.session_id, previous_session_id);
        assert!(!controller.session_setup.session_created);
        assert!(controller.queued_prompts.is_empty());
        assert!(controller.pending_tree_summary_target.is_none());
        assert!(controller.pending_tree_custom_prompt_target.is_none());
        assert!(controller.pending_images.is_empty());
        assert!(controller.retry_status.is_none());
        assert!(!controller.manual_compaction_in_progress);
        assert_eq!(controller.manual_compaction_generation, 8);
        assert!(cancel.is_cancelled());

        let commands = drain_commands(&mut command_rx);
        assert!(
            commands
                .iter()
                .any(|command| matches!(command, TuiCommand::SetLocalActionActive(false)))
        );
        assert!(
            commands
                .iter()
                .any(|command| matches!(command, TuiCommand::SetInput(text) if text.is_empty()))
        );
        assert!(commands.iter().any(|command| matches!(command, TuiCommand::SetTranscript(transcript) if transcript.root_blocks().is_empty())));
        assert!(commands.iter().any(|command| matches!(command, TuiCommand::PushNote { text, .. } if text == "New session started")));
    }

    #[test]
    fn open_resume_menu_lists_named_and_unnamed_sessions() {
        let (mut controller, mut command_rx, _tempdir) = build_test_controller();
        let cwd = controller.session_setup.tool_ctx.cwd.display().to_string();
        let named_session = store::create_session(&controller.session_setup.conn, &cwd)
            .expect("create named session");
        store::set_session_name(
            &controller.session_setup.conn,
            &named_session,
            Some("named session"),
        )
        .expect("name session");
        let unnamed_session = store::create_session(&controller.session_setup.conn, &cwd)
            .expect("create unnamed session");

        controller.open_resume_menu().expect("open resume menu");

        let commands = drain_commands(&mut command_rx);
        let (menu_id, title, items) = commands
            .into_iter()
            .find_map(|command| match command {
                TuiCommand::OpenSelectMenu {
                    menu_id,
                    title,
                    items,
                    ..
                } => Some((menu_id, title, items)),
                _ => None,
            })
            .expect("resume menu command");

        assert_eq!(menu_id, RESUME_SESSION_MENU_ID);
        assert_eq!(title, "Resume session");
        assert!(
            items
                .iter()
                .any(|item| item.label == "named session" && item.value == named_session)
        );
        assert!(items.iter().any(|item| item.label
            == unnamed_session.chars().take(8).collect::<String>()
            && item.value == unnamed_session));
        assert!(items.iter().all(|item| {
            item.detail
                .as_deref()
                .is_some_and(|detail| detail.contains("entries •"))
        }));
    }

    #[tokio::test]
    async fn handle_resume_session_clears_stale_state_and_reports_success() {
        let (mut controller, mut command_rx, _tempdir) = build_test_controller();
        let cwd = controller.session_setup.tool_ctx.cwd.display().to_string();
        let session_id =
            store::create_session(&controller.session_setup.conn, &cwd).expect("create session");
        let user_entry = SessionEntry::Message {
            base: EntryBase {
                id: EntryId::generate(),
                parent_id: None,
                timestamp: Utc::now(),
            },
            message: AgentMessage::User(UserMessage {
                content: vec![ContentBlock::Text {
                    text: "resume me".to_string(),
                }],
                timestamp: Utc::now().timestamp_millis(),
            }),
        };
        store::append_entry(&controller.session_setup.conn, &session_id, &user_entry)
            .expect("append user entry");

        controller.pending_tree_summary_target = Some("tree-1".to_string());
        controller.pending_tree_custom_prompt_target = Some("tree-2".to_string());
        controller.pending_images.push(PendingImage {
            data: "abcd".to_string(),
            mime_type: "image/png".to_string(),
        });
        controller.queued_prompts = VecDeque::from([QueuedPrompt::Hidden("queued".to_string())]);
        controller.streaming = true;
        controller.retry_status = Some("retrying".to_string());
        controller.manual_compaction_in_progress = true;
        controller.manual_compaction_generation = 11;
        let cancel = CancellationToken::new();
        controller.local_action_cancel = Some(cancel.clone());

        controller
            .handle_resume_session(&session_id)
            .await
            .expect("resume session");

        assert_eq!(controller.session_setup.session_id, session_id);
        assert!(controller.session_setup.session_created);
        assert_eq!(
            controller.options.session_id.as_deref(),
            Some(session_id.as_str())
        );
        assert!(controller.pending_tree_summary_target.is_none());
        assert!(controller.pending_tree_custom_prompt_target.is_none());
        assert!(controller.pending_images.is_empty());
        assert!(controller.queued_prompts.is_empty());
        assert!(!controller.streaming);
        assert!(controller.retry_status.is_none());
        assert!(!controller.manual_compaction_in_progress);
        assert_eq!(controller.manual_compaction_generation, 12);
        assert!(cancel.is_cancelled());

        let commands = drain_commands(&mut command_rx);
        assert!(commands.iter().any(|command| matches!(command, TuiCommand::SetStatusLine(text) if text == "Resuming session...")));
        assert!(
            commands
                .iter()
                .any(|command| matches!(command, TuiCommand::SetLocalActionActive(true)))
        );
        assert!(
            commands
                .iter()
                .any(|command| matches!(command, TuiCommand::SetTranscriptWithToolStates { .. }))
        );
        assert!(
            commands
                .iter()
                .any(|command| matches!(command, TuiCommand::SetInput(text) if text.is_empty()))
        );
        assert!(commands.iter().any(|command| matches!(command, TuiCommand::SetStatusLine(text) if text == "Resumed session")));
    }
}
