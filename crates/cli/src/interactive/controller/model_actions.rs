use super::*;

impl InteractiveMode {
    pub(super) fn show_settings_selector(&mut self) {
        self.show_status("TODO: settings selector");
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
                self.show_status("Canceled model selector");
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
                self.handle_auth_login(&provider);
            }
            Some(AuthSelectorAction::Logout(provider)) => {
                self.ui.tui.hide_overlay();
                self.handle_auth_logout(&provider);
            }
            Some(AuthSelectorAction::Cancelled) => {
                self.ui.tui.hide_overlay();
                self.show_status("Canceled");
            }
            _ => {}
        }
    }

    pub(super) fn show_auth_selector(&mut self, mode: AuthSelectorMode) {
        let overlay = Box::new(AuthSelectorOverlay::new(mode));
        self.ui.tui.show_overlay(overlay);
        let label = match mode {
            AuthSelectorMode::Login => "login / re-auth",
            AuthSelectorMode::Logout => "logout",
        };
        self.show_status(format!("Select provider to {label}"));
    }

    fn handle_auth_login(&mut self, provider: &str) {
        use super::super::auth_selector_overlay::{auth_method_for, AuthMethod};
        use crate::login::auth_source;

        let method = auth_method_for(provider);
        let source = auth_source(provider);
        let verb = if source.is_some() { "Re-auth" } else { "Login" };
        let source_note = match source {
            Some(s) => format!(" (current: via {})", s.label()),
            None => String::new(),
        };

        match method {
            AuthMethod::OAuth => {
                self.start_oauth_flow(provider, verb, &source_note);
            }
            AuthMethod::ApiKey => {
                self.show_status(format!(
                    "{verb} {provider}{source_note} -- paste API key and press Enter (Esc to cancel)"
                ));
                self.streaming.pending_auth_provider = Some(provider.to_string());
            }
        }
    }

    /// Start the OAuth browser + callback flow for a provider.
    ///
    /// Opens browser, starts callback server, and also allows manual code paste.
    /// The editor is used for manual paste input. Results arrive via poll_oauth_result.
    fn start_oauth_flow(&mut self, provider: &str, verb: &str, source_note: &str) {
        use crate::oauth::{self, OAuthCallbacks};

        // Oneshot so the user can paste the code/URL manually.
        let (manual_tx, manual_rx) = tokio::sync::oneshot::channel::<String>();

        self.streaming.pending_auth_provider = Some(provider.to_string());
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
            self.show_status(format!(
                "{verb} {provider}{source_note}\n\
                 Open this URL in a browser:\n\
                 \n\
                   {url}\n\
                 \n\
                 Then paste the redirect URL or code here and press Enter.\n\
                 Press Esc to cancel."
            ));
        } else {
            self.show_status(format!(
                "{verb} {provider}{source_note} — starting OAuth flow… (Esc to cancel)"
            ));
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
                let provider = self
                    .streaming
                    .pending_auth_provider
                    .take()
                    .unwrap_or_default();
                self.streaming.pending_oauth_result_rx = None;
                self.streaming.pending_oauth_manual_tx = None;

                match crate::login::save_oauth_credentials(&provider, &creds) {
                    Ok(()) => {
                        // Update live key if this is the active provider.
                        let model_provider = &self.session_setup.model.provider;
                        let matches = model_provider == &provider
                            || (provider == "openai-codex" && model_provider == "openai");
                        if matches {
                            self.session_setup.api_key = creds.access.clone();
                        }
                        self.show_status(format!(
                            "Logged in to {provider}. Credentials saved."
                        ));
                    }
                    Err(e) => {
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
                self.show_warning(format!("Login failed: {err}"));
            }
            Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {
                // Still waiting.
            }
            Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {
                self.streaming.pending_auth_provider = None;
                self.streaming.pending_oauth_result_rx = None;
                self.streaming.pending_oauth_manual_tx = None;
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

        let key = key_text.trim();

        // Empty or just "/" — cancel.
        if key.is_empty() || key == "/" {
            self.cancel_pending_auth();
            self.show_status("Login canceled.");
            return;
        }

        // If there's a pending OAuth manual-paste channel, forward the text
        // to the OAuth flow which will parse the code from it.
        if let Some(tx) = self.streaming.pending_oauth_manual_tx.take() {
            if tx.send(key.to_string()).is_ok() {
                self.show_status(format!("Exchanging tokens for {provider}…"));
                // Result will arrive via poll_oauth_result.
                return;
            }
            // Channel closed — OAuth task already finished (maybe callback server got it).
            // Fall through to check if it was an error.
            if self.streaming.pending_oauth_result_rx.is_some() {
                // Let poll_oauth_result handle it on next tick.
                self.show_status("Processing…");
                return;
            }
        }

        // No OAuth channel — this is a plain API-key paste.
        self.streaming.pending_auth_provider = None;

        match crate::login::save_api_key(&provider, key) {
            Ok(()) => {
                self.show_status(format!("Saved API key for {provider}"));
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
        if let Some(tx) = self.streaming.pending_oauth_manual_tx.take() {
            // Dropping tx cancels the manual input in the OAuth flow.
            drop(tx);
        }
        self.streaming.pending_oauth_result_rx = None;
    }

    fn handle_auth_logout(&mut self, provider: &str) {
        match crate::login::remove_auth(provider) {
            Ok(true) => {
                self.show_status(format!("Logged out from {provider}"));
                self.rebuild_footer();
            }
            Ok(false) => {
                let source = crate::login::auth_source(provider);
                match source {
                    Some(crate::login::AuthSource::EnvVar) => {
                        self.show_warning(format!(
                            "{provider}: credentials come from environment variable. Unset it in your shell."
                        ));
                    }
                    _ => {
                        self.show_status(format!("{provider}: not logged in"));
                    }
                }
            }
            Err(e) => {
                self.show_warning(format!("Logout failed: {e}"));
            }
        }
    }
}
