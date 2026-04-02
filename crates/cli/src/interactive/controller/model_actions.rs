use super::*;

fn persist_retry_settings(
    enabled: bool,
    max_retries: u32,
    base_delay_ms: u64,
    max_delay_ms: u64,
) -> Result<(), String> {
    let mut settings = bb_core::settings::Settings::load_global();
    settings.retry.enabled = enabled;
    settings.retry.max_retries = max_retries.max(1);
    settings.retry.base_delay_ms = base_delay_ms.max(1_000);
    settings.retry.max_delay_ms = max_delay_ms.max(settings.retry.base_delay_ms);
    settings.save_global().map_err(|e| e.to_string())
}

impl InteractiveMode {
    pub(super) fn show_settings_selector(&mut self) {
        self.clear_status();
        let thinking = &self.session_setup.thinking_level;
        let items = vec![
            SettingItem {
                id: "thinking".into(),
                label: "Thinking level".into(),
                description: "Reasoning depth for the model".into(),
                current_value: thinking.clone(),
                values: vec!["off".into(), "low".into(), "medium".into(), "high".into(), "xhigh".into()],
            },
            SettingItem {
                id: "autocompact".into(),
                label: "Auto-compact".into(),
                description: "Automatically compact context when it gets large".into(),
                current_value: "true".into(),
                values: vec!["true".into(), "false".into()],
            },
            SettingItem {
                id: "retry-enabled".into(),
                label: "Auto-retry".into(),
                description: "Retry retryable provider errors automatically".into(),
                current_value: if self.session_setup.retry_enabled { "true".into() } else { "false".into() },
                values: vec!["true".into(), "false".into()],
            },
            SettingItem {
                id: "retry-max".into(),
                label: "Retry attempts".into(),
                description: "Maximum number of automatic retry attempts".into(),
                current_value: self.session_setup.retry_max_retries.to_string(),
                values: vec!["1".into(), "2".into(), "3".into(), "4".into(), "5".into()],
            },
            SettingItem {
                id: "retry-delay".into(),
                label: "Retry base delay".into(),
                description: "Initial retry backoff delay".into(),
                current_value: format!("{}s", self.session_setup.retry_base_delay_ms / 1000),
                values: vec!["1s".into(), "2s".into(), "5s".into(), "10s".into()],
            },
            SettingItem {
                id: "retry-max-delay".into(),
                label: "Retry max delay".into(),
                description: "Maximum allowed server-requested retry delay".into(),
                current_value: format!("{}s", self.session_setup.retry_max_delay_ms / 1000),
                values: vec!["10s".into(), "30s".into(), "60s".into(), "120s".into()],
            },
            SettingItem {
                id: "tool-expand".into(),
                label: "Expand tool output".into(),
                description: "Show full tool output by default".into(),
                current_value: if self.interaction.tool_output_expanded { "true".into() } else { "false".into() },
                values: vec!["true".into(), "false".into()],
            },
            SettingItem {
                id: "hide-thinking".into(),
                label: "Hide thinking blocks".into(),
                description: "Collapse thinking blocks in assistant messages".into(),
                current_value: if self.streaming.hide_thinking_block { "true".into() } else { "false".into() },
                values: vec!["true".into(), "false".into()],
            },
        ];

        let overlay = Box::new(SettingsOverlay::new(items));
        self.ui.tui.show_overlay(overlay);
        self.ui.tui.force_render();
    }

    pub(super) fn handle_model_command(&mut self, search: Option<&str>) {
        let search_term = search.unwrap_or_default();
        if let Some(model) = self.find_exact_model_match(search_term) {
            self.apply_model_selection(model);
        } else {
            self.show_model_selector(Some(search_term));
        }
    }

    pub(super) fn get_model_candidates(&self) -> Vec<Model> {
        let current_provider = self.session_setup.model.provider.clone();
        let available = crate::login::authenticated_providers();
        ModelRegistry::new()
            .list()
            .iter()
            .filter(|model| {
                available.iter().any(|provider| provider == &model.provider)
                    || model.provider == current_provider
            })
            .cloned()
            .collect()
    }

    pub(super) fn find_exact_model_match(&self, search_term: &str) -> Option<Model> {
        let needle = search_term.trim().to_ascii_lowercase();
        self.get_model_candidates().into_iter().find(|model| {
            let provider_id = format!("{}/{}", model.provider, model.id).to_ascii_lowercase();
            let provider_colon_id =
                format!("{}:{}", model.provider, model.id).to_ascii_lowercase();
            model.id.eq_ignore_ascii_case(&needle)
                || model.name.eq_ignore_ascii_case(&needle)
                || provider_id == needle
                || provider_colon_id == needle
        })
    }

    pub(super) fn apply_model_selection(&mut self, model: Model) {
        let api_key = crate::login::resolve_api_key(&model.provider).unwrap_or_default();
        let base_url = model.base_url.clone().unwrap_or_else(|| match model.api {
            ApiType::AnthropicMessages => "https://api.anthropic.com".to_string(),
            ApiType::GoogleGenerative => {
                "https://generativelanguage.googleapis.com".to_string()
            }
            _ => "https://api.openai.com/v1".to_string(),
        });
        let new_provider: std::sync::Arc<dyn bb_provider::Provider> = match model.api {
            ApiType::AnthropicMessages => {
                std::sync::Arc::new(bb_provider::anthropic::AnthropicProvider::new())
            }
            ApiType::GoogleGenerative => {
                std::sync::Arc::new(bb_provider::google::GoogleProvider::new())
            }
            _ => std::sync::Arc::new(bb_provider::openai::OpenAiProvider::new()),
        };
        let display = format!("{}/{}", model.provider, model.id);

        self.controller
            .runtime_host
            .session_mut()
            .set_model(ModelRef {
                provider: model.provider.clone(),
                id: model.id.clone(),
                reasoning: model.reasoning,
            });
        self.controller.runtime_host.runtime_mut().model = Some(RuntimeModelRef {
            provider: model.provider.clone(),
            id: model.id.clone(),
            context_window: model.context_window as usize,
        });
        self.session_setup.model = model;
        self.session_setup.provider = new_provider;
        self.session_setup.api_key = api_key;
        self.session_setup.base_url = base_url;
        self.options.model_display = Some(display.clone());
        self.show_status(format!("Model: {display}"));
        self.rebuild_footer();
    }

    pub(super) fn process_overlay_actions(&mut self) {
        // Check model selector overlay
        let model_action = self
            .ui
            .tui
            .topmost_overlay_as_mut::<ModelSelectorOverlay>()
            .and_then(|overlay| overlay.take_action());

        match model_action {
            Some(ModelSelectorOverlayAction::Selected(selection)) => {
                self.ui.tui.hide_overlay();
                self.clear_status();
                if let Some(model) = self
                    .get_model_candidates()
                    .into_iter()
                    .find(|m| {
                        m.provider == selection.provider && m.id == selection.model_id
                    })
                {
                    self.apply_model_selection(model);
                } else {
                    self.show_warning(format!(
                        "Model not found: {}/{}",
                        selection.provider, selection.model_id
                    ));
                }
                return;
            }
            Some(ModelSelectorOverlayAction::Cancelled) => {
                self.ui.tui.hide_overlay();
                self.clear_status();
                self.refresh_ui();
                return;
            }
            None => {}
        }

        // Check auth selector overlay
        let auth_action = self
            .ui
            .tui
            .topmost_overlay_as_mut::<AuthSelectorOverlay>()
            .map(|overlay| overlay.action().clone());

        match auth_action {
            Some(AuthSelectorAction::Login(provider)) => {
                self.ui.tui.hide_overlay();
                self.clear_status();
                self.handle_auth_login(&provider);
            }
            Some(AuthSelectorAction::Logout(provider)) => {
                self.ui.tui.hide_overlay();
                self.clear_status();
                self.handle_auth_logout(&provider);
            }
            Some(AuthSelectorAction::Cancelled) => {
                self.ui.tui.hide_overlay();
                self.clear_status();
                self.refresh_ui();
            }
            _ => {}
        }

        // Check session selector overlay
        let session_action = self
            .ui
            .tui
            .topmost_overlay_as_mut::<SessionSelectorOverlay>()
            .map(|overlay| overlay.action().clone());

        match session_action {
            Some(SessionSelectorAction::Selected(id)) => {
                self.ui.tui.hide_overlay();
                self.clear_status();
                if self.interaction.pending_fork {
                    self.interaction.pending_fork = false;
                    self.handle_fork_from_entry(&id);
                } else {
                    self.handle_resume_session(&id);
                }
            }
            Some(SessionSelectorAction::Cancelled) => {
                self.ui.tui.hide_overlay();
                self.clear_status();
                self.interaction.pending_fork = false;
                self.refresh_ui();
            }
            _ => {}
        }

        // Check tree selector overlay
        let tree_action = self
            .ui
            .tui
            .topmost_overlay_as_mut::<TreeSelectorOverlay>()
            .map(|overlay| overlay.action().clone());

        match tree_action {
            Some(TreeSelectorAction::Selected(entry_id)) => {
                self.ui.tui.hide_overlay();
                self.clear_status();
                self.handle_tree_navigate(&entry_id);
            }
            Some(TreeSelectorAction::Cancelled) => {
                self.ui.tui.hide_overlay();
                self.clear_status();
                self.refresh_ui();
            }
            _ => {}
        }

        // Check settings overlay
        let settings_action = self
            .ui
            .tui
            .topmost_overlay_as_mut::<SettingsOverlay>()
            .map(|overlay| overlay.action().clone());

        match settings_action {
            Some(SettingsAction::Changed(id, value)) => {
                self.apply_setting(&id, &value);
            }
            Some(SettingsAction::Cancelled) => {
                self.ui.tui.hide_overlay();
                self.clear_status();
                self.refresh_ui();
            }
            _ => {}
        }
    }

    fn apply_setting(&mut self, id: &str, value: &str) {
        match id {
            "thinking" => {
                self.session_setup.thinking_level = value.to_string();
                let level = match value {
                    "off" => ThinkingLevel::Off,
                    "low" => ThinkingLevel::Low,
                    "medium" => ThinkingLevel::Medium,
                    "high" => ThinkingLevel::High,
                    "xhigh" => ThinkingLevel::XHigh,
                    _ => ThinkingLevel::Medium,
                };
                self.controller.runtime_host.session_mut().set_thinking_level(level);
                self.rebuild_footer();
            }
            "autocompact" => {}
            "retry-enabled" => {
                self.session_setup.retry_enabled = value == "true";
            }
            "retry-max" => {
                if let Ok(parsed) = value.parse::<u32>() {
                    self.session_setup.retry_max_retries = parsed.max(1);
                }
            }
            "retry-delay" => {
                let secs = value.trim_end_matches('s').parse::<u64>().unwrap_or(1);
                self.session_setup.retry_base_delay_ms = secs.max(1) * 1000;
                if self.session_setup.retry_max_delay_ms < self.session_setup.retry_base_delay_ms {
                    self.session_setup.retry_max_delay_ms = self.session_setup.retry_base_delay_ms;
                }
            }
            "retry-max-delay" => {
                let secs = value.trim_end_matches('s').parse::<u64>().unwrap_or(60);
                self.session_setup.retry_max_delay_ms = secs.max(1) * 1000;
                if self.session_setup.retry_max_delay_ms < self.session_setup.retry_base_delay_ms {
                    self.session_setup.retry_max_delay_ms = self.session_setup.retry_base_delay_ms;
                }
            }
            "tool-expand" => {
                self.interaction.tool_output_expanded = value == "true";
                self.set_chat_tools_expanded(self.interaction.tool_output_expanded);
                self.rebuild_pending_container();
            }
            "hide-thinking" => {
                self.streaming.hide_thinking_block = value == "true";
                let hide = self.streaming.hide_thinking_block;
                let label = self.streaming.hidden_thinking_label.clone();
                let tools_expanded = self.interaction.tool_output_expanded;
                if let Some(component) = self.render_state_mut().streaming_component.as_mut() {
                    component.set_hide_thinking_block(hide);
                    component.set_hidden_thinking_label(label.clone());
                }
                for component in self.render_state_mut().pending_tools.values_mut() {
                    component.set_expanded(tools_expanded);
                }
                self.rebuild_chat_from_session_with_live_components();
                self.rebuild_pending_container();
            }
            _ => return,
        };

        if matches!(id, "retry-enabled" | "retry-max" | "retry-delay" | "retry-max-delay") {
            if let Err(e) = persist_retry_settings(
                self.session_setup.retry_enabled,
                self.session_setup.retry_max_retries,
                self.session_setup.retry_base_delay_ms,
                self.session_setup.retry_max_delay_ms,
            ) {
                self.show_error(format!("Failed to persist retry settings: {e}"));
                self.refresh_ui();
                return;
            }
        }

        self.refresh_ui();
    }

    pub(super) fn show_auth_selector(&mut self, mode: AuthSelectorMode) {
        let overlay = Box::new(AuthSelectorOverlay::new(mode));
        self.clear_status();
        self.ui.tui.show_overlay(overlay);
        self.ui.tui.force_render();
    }

    fn handle_auth_login(&mut self, provider: &str) {
        use super::super::auth_selector_overlay::{
            auth_display_name_for, auth_method_for, AuthMethod,
        };
        use crate::login::auth_source;

        let method = auth_method_for(provider);
        let display_name = auth_display_name_for(provider).to_string();
        let source = auth_source(provider);
        let verb = if source.is_some() { "Re-auth" } else { "Login" };
        let source_note = match source {
            Some(s) => format!(" (current: via {})", s.label()),
            None => String::new(),
        };

        match method {
            AuthMethod::OAuth => {
                self.start_oauth_flow(provider, &display_name, verb, &source_note);
            }
            AuthMethod::ApiKey => {
                self.streaming.pending_auth_provider = Some(provider.to_string());
                self.streaming.pending_auth_display_name = Some(display_name.clone());
                self.streaming.pending_auth_url = None;
                self.streaming.pending_auth_message = Some(format!(
                    "{verb} {display_name}{source_note}\nPaste API key below and press Enter.\nPress Esc to cancel."
                ));
                self.show_status(format!("{verb} {display_name}{source_note}"));
            }
        }
    }

    /// Start the OAuth browser + callback flow for a provider.
    ///
    /// Opens browser, starts callback server, and also allows manual code paste.
    /// The editor is used for manual paste input. Results arrive via poll_oauth_result.
    fn start_oauth_flow(&mut self, provider: &str, display_name: &str, verb: &str, source_note: &str) {
        use crate::oauth::{self, OAuthCallbacks};

        // Oneshot so the user can paste the code/URL manually.
        let (manual_tx, manual_rx) = tokio::sync::oneshot::channel::<String>();

        self.streaming.pending_auth_provider = Some(provider.to_string());
        self.streaming.pending_auth_display_name = Some(display_name.to_string());
        self.streaming.pending_auth_url = None;
        self.streaming.pending_oauth_manual_tx = Some(manual_tx);

        let provider_for_task = provider.to_string();

        // Channel so the on_auth callback can send the URL back.
        let (url_tx, url_rx) = std::sync::mpsc::channel::<String>();

        let callbacks = OAuthCallbacks {
            on_auth: Box::new(move |url: String| {
                let _ = url_tx.send(url.clone());
                // Best-effort open browser. Suppress all output.
                #[cfg(target_os = "macos")]
                let _ = std::process::Command::new("open")
                    .arg(&url)
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn();
                #[cfg(not(target_os = "macos"))]
                let _ = std::process::Command::new("xdg-open")
                    .arg(&url)
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn();
            }),
            on_manual_input: Some(manual_rx),
            on_progress: None,
        };

        // Spawn OAuth flow on the runtime.
        let result_tx = {
            let (tx, rx) =
                tokio::sync::oneshot::channel::<Result<oauth::OAuthCredentials, String>>();
            self.streaming.pending_oauth_result_rx = Some(rx);
            tx
        };

        tokio::spawn(async move {
            let result = match provider_for_task.as_str() {
                "anthropic" => oauth::login_anthropic(callbacks).await,
                "openai-codex" => oauth::login_openai_codex(callbacks).await,
                _ => Err(anyhow::anyhow!(
                    "No OAuth flow for provider {provider_for_task}"
                )),
            };
            let _ = result_tx.send(result.map_err(|e| e.to_string()));
        });

        // Show the auth URL in the TUI so headless users can copy it.
        // Use a short timeout — the on_auth callback fires almost immediately.
        if let Ok(url) = url_rx.recv_timeout(std::time::Duration::from_secs(3)) {
            self.streaming.pending_auth_url = Some(url);
            self.streaming.pending_auth_message = Some(
                "Open this URL in a browser.\nThen paste the redirect URL or code below and press Enter.\nPress Esc to cancel."
                    .to_string(),
            );
            self.show_status(format!("{verb} {display_name}{source_note}"));
        } else {
            self.streaming.pending_auth_message = Some(format!(
                "Starting OAuth flow for {display_name}…\nPress Esc to cancel."
            ));
            self.show_status(format!("{verb} {display_name}{source_note}"));
        }
    }

    /// Poll for the result of a pending OAuth flow (non-blocking).
    /// Called from the main event loop so the TUI stays responsive.
    pub(super) fn poll_oauth_result(&mut self) {
        let rx = match self.streaming.pending_oauth_result_rx.as_mut() {
            Some(rx) => rx,
            None => return,
        };
        match rx.try_recv() {
            Ok(Ok(creds)) => {
                let provider = match self.streaming.pending_auth_provider.take() {
                    Some(p) if !p.is_empty() => p,
                    _ => {
                        // Provider name was lost — should not happen.
                        self.streaming.pending_oauth_result_rx = None;
                        self.streaming.pending_oauth_manual_tx = None;
                        self.streaming.pending_auth_url = None;
                        self.streaming.pending_auth_message = None;
                        self.streaming.pending_auth_display_name = None;
                        self.show_warning("OAuth completed but provider name was lost.");
                        return;
                    }
                };
                let display_name = self
                    .streaming
                    .pending_auth_display_name
                    .clone()
                    .unwrap_or_else(|| provider.clone());
                self.streaming.pending_oauth_result_rx = None;
                self.streaming.pending_oauth_manual_tx = None;
                self.streaming.pending_auth_url = None;

                match crate::login::save_oauth_credentials(&provider, &creds) {
                    Ok(()) => {
                        // Update live key if this is the active provider.
                        let model_provider = &self.session_setup.model.provider;
                        let matches = model_provider == &provider
                            || (provider == "openai-codex" && model_provider == "openai");
                        if matches {
                            self.session_setup.api_key = creds.access.clone();
                        }
                        self.streaming.pending_auth_message = None;
                        self.streaming.pending_auth_display_name = None;
                        self.show_status(format!(
                            "Logged in to {display_name}. Credentials saved to {}. Verifying…",
                            crate::login::auth_path().display()
                        ));
                        // Queue a verification prompt so user sees the login works.
                        self.streaming.pending_oauth_verify_provider =
                            Some(provider.clone());
                    }
                    Err(e) => {
                        self.streaming.pending_auth_message = None;
                        self.streaming.pending_auth_display_name = None;
                        self.show_warning(format!(
                            "OAuth succeeded but failed to save tokens: {e}"
                        ));
                    }
                }
                self.rebuild_footer();
            }
            Ok(Err(err)) => {
                self.streaming.pending_auth_provider = None;
                self.streaming.pending_oauth_result_rx = None;
                self.streaming.pending_oauth_manual_tx = None;
                self.streaming.pending_auth_url = None;
                self.streaming.pending_auth_message = None;
                self.streaming.pending_auth_display_name = None;
                self.show_warning(format!("Login failed: {err}"));
            }
            Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {
                // Still waiting.
            }
            Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {
                self.streaming.pending_auth_provider = None;
                self.streaming.pending_oauth_result_rx = None;
                self.streaming.pending_oauth_manual_tx = None;
                self.streaming.pending_auth_url = None;
                self.streaming.pending_auth_message = None;
                self.streaming.pending_auth_display_name = None;
                // Don't show warning — the flow may have completed normally
                // and the channel was dropped after sending.
            }
        }
    }

    /// Called when user submits text while `pending_auth_provider` is set.
    ///
    /// For OAuth providers: forwards the pasted code/URL to the OAuth flow.
    /// For API key providers: saves the key directly.
    pub(super) fn finish_auth_login(&mut self, key_text: &str) {
        let provider = match self.streaming.pending_auth_provider.as_ref() {
            Some(p) => p.clone(),
            None => return,
        };
        let display_name = self
            .streaming
            .pending_auth_display_name
            .clone()
            .unwrap_or_else(|| provider.clone());

        let key = key_text.trim();

        // Empty or just "/" — cancel.
        if key.is_empty() || key == "/" {
            self.cancel_pending_auth();
            self.refresh_ui();
            return;
        }

        // If there's a pending OAuth manual-paste channel, forward the text
        // to the OAuth flow which will parse the code from it.
        if let Some(tx) = self.streaming.pending_oauth_manual_tx.take() {
            if tx.send(key.to_string()).is_ok() {
                self.streaming.pending_auth_message = Some(format!("Exchanging tokens for {display_name}…"));
                self.show_status(format!("Exchanging tokens for {display_name}…"));
                // Result will arrive via poll_oauth_result.
                return;
            }
            // Channel closed — OAuth task already finished (maybe callback server got it).
            // Fall through to check if it was an error.
            if self.streaming.pending_oauth_result_rx.is_some() {
                // Let poll_oauth_result handle it on next tick.
                self.streaming.pending_auth_message = Some("Processing…".to_string());
                self.show_status("Processing…");
                return;
            }
        }

        // No OAuth channel — this is a plain API-key paste.
        self.streaming.pending_auth_provider = None;
        self.streaming.pending_auth_display_name = None;
        self.streaming.pending_auth_url = None;
        self.streaming.pending_auth_message = None;

        match crate::login::save_api_key(&provider, key) {
            Ok(()) => {
                self.show_status(format!("Saved API key for {display_name}"));
                if self.session_setup.provider.name() == provider
                    || (provider == "openai-codex"
                        && self.session_setup.provider.name() == "openai")
                {
                    self.session_setup.api_key = key.to_string();
                }
                self.rebuild_footer();
            }
            Err(e) => {
                self.show_warning(format!("Failed to save key: {e}"));
            }
        }
    }

    /// Cancel any pending auth flow (OAuth or API key).
    pub(super) fn cancel_pending_auth(&mut self) {
        self.streaming.pending_auth_provider = None;
        self.streaming.pending_auth_display_name = None;
        self.streaming.pending_auth_url = None;
        self.streaming.pending_auth_message = None;
        if let Some(tx) = self.streaming.pending_oauth_manual_tx.take() {
            // Dropping tx cancels the manual input in the OAuth flow.
            drop(tx);
        }
        self.streaming.pending_oauth_result_rx = None;
    }

    fn handle_auth_logout(&mut self, provider: &str) {
        let display_name = super::super::auth_selector_overlay::auth_display_name_for(provider);
        match crate::login::remove_auth(provider) {
            Ok(true) => {
                // Clear the live session key if it matches the logged-out provider.
                let model_provider = &self.session_setup.model.provider;
                let matches = model_provider == provider
                    || (provider == "openai-codex" && model_provider == "openai")
                    || (provider == "openai" && model_provider == "openai-codex")
                    || (provider == "anthropic" && model_provider == "anthropic");
                if matches {
                    self.session_setup.api_key.clear();
                }
                self.show_status(format!("Logged out of {display_name}"));
                self.rebuild_footer();
            }
            Ok(false) => {
                let source = crate::login::auth_source(provider);
                match source {
                    Some(crate::login::AuthSource::EnvVar) => {
                        self.show_warning(format!(
                            "{display_name}: credentials come from environment variable. Unset it in your shell."
                        ));
                    }
                    _ => {
                        self.show_status(format!("{display_name}: not logged in"));
                    }
                }
            }
            Err(e) => {
                self.show_warning(format!("Logout failed: {e}"));
            }
        }
    }
}
