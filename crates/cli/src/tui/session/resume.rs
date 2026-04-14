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
                        self.runtime_host.runtime_mut().model = Some(RuntimeModelRef {
                            provider: model.provider.clone(),
                            id: model.id.clone(),
                            context_window: model.context_window as usize,
                        });
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
