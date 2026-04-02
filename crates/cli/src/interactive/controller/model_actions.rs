use super::*;

impl InteractiveMode {
    pub(super) fn show_settings_selector(&mut self) {
        let _ = self.controller.commands.show_settings_selector();
        self.show_placeholder("settings selector");
    }

    pub(super) fn handle_model_command(&mut self, search_term: Option<&str>) {
        let Some(search_term) = search_term.map(str::trim).filter(|s| !s.is_empty()) else {
            self.show_model_selector(None);
            return;
        };

        if let Some(model) = self.find_exact_model_match(search_term) {
            self.apply_model_selection(model);
            return;
        }

        self.show_model_selector(Some(search_term));
    }

    pub(super) fn build_model_registry(&self) -> ModelRegistry {
        let mut registry = ModelRegistry::new();
        let settings = bb_core::settings::Settings::load_merged(&self.controller.runtime_host.cwd());
        registry.load_custom_models(&settings);
        registry
    }

    pub(super) fn get_model_candidates(&self) -> Vec<Model> {
        let current_provider = self.session_setup.model.provider.clone();
        let available = crate::login::authenticated_providers();
        let has_any_available = !available.is_empty();

        self.build_model_registry()
            .list()
            .iter()
            .filter(|model| {
                !has_any_available
                    || available.iter().any(|provider| provider == &model.provider)
                    || model.provider == current_provider
            })
            .cloned()
            .collect()
    }

    pub(super) fn find_exact_model_match(&self, search_term: &str) -> Option<Model> {
        let needle = search_term.trim().to_ascii_lowercase();
        self.get_model_candidates().into_iter().find(|model| {
            let provider_id = format!("{}/{}", model.provider, model.id).to_ascii_lowercase();
            let provider_colon_id = format!("{}:{}", model.provider, model.id).to_ascii_lowercase();
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
            ApiType::GoogleGenerative => "https://generativelanguage.googleapis.com".to_string(),
            _ => "https://api.openai.com/v1".to_string(),
        });
        let new_provider: std::sync::Arc<dyn bb_provider::Provider> = match model.api {
            ApiType::AnthropicMessages => std::sync::Arc::new(bb_provider::anthropic::AnthropicProvider::new()),
            ApiType::GoogleGenerative => std::sync::Arc::new(bb_provider::google::GoogleProvider::new()),
            _ => std::sync::Arc::new(bb_provider::openai::OpenAiProvider::new()),
        };
        let display = format!("{}/{}", model.provider, model.id);

        self.controller.runtime_host.session_mut().set_model(ModelRef {
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
            .ui.tui
            .topmost_overlay_as_mut::<ModelSelectorOverlay>()
            .and_then(|overlay| overlay.take_action());

        match model_action {
            Some(ModelSelectorOverlayAction::Selected(selection)) => {
                self.ui.tui.hide_overlay();
                if let Some(model) = self
                    .get_model_candidates()
                    .into_iter()
                    .find(|m| m.provider == selection.provider && m.id == selection.model_id)
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
            .ui.tui
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
        use crate::login::{auth_source, AuthSource};

        // Put the editor into "api key input" mode.
        // We stash the provider name and switch the submit route so that
        // the next Enter delivers the text to save_api_key instead of
        // sending it as a chat prompt.
        let source = auth_source(provider);
        let verb = if source.is_some() { "Re-auth" } else { "Login" };
        let source_note = match source {
            Some(s) => format!(" (current: via {})", s.label()),
            None => String::new(),
        };

        // Show instructions in status and pre-fill editor hint
        self.show_status(format!(
            "{verb} {provider}{source_note} -- paste API key and press Enter (Esc to cancel)"
        ));

        // Store provider for the pending key entry
        self.streaming.pending_auth_provider = Some(provider.to_string());
    }

    /// Called when user submits text while pending_auth_provider is set.
    pub(super) fn finish_auth_login(&mut self, key_text: &str) {
        let provider = match self.streaming.pending_auth_provider.take() {
            Some(p) => p,
            None => return,
        };

        let key = key_text.trim();
        if key.is_empty() {
            self.show_status("No key entered, login canceled.");
            return;
        }

        match crate::login::save_api_key(&provider, key) {
            Ok(()) => {
                self.show_status(format!("Saved API key for {provider}"));
                // If this provider matches our current model's provider, update the live key
                if self.session_setup.provider.name() == provider {
                    self.session_setup.api_key = key.to_string();
                }
                self.rebuild_footer();
            }
            Err(e) => {
                self.show_warning(format!("Failed to save key: {e}"));
            }
        }
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
