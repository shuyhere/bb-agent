use super::*;

#[derive(Default)]
struct NormalizedModelSelection {
    provider_filter: Option<String>,
    match_term: String,
    thinking_override: Option<ThinkingLevel>,
}

impl TuiController {
    pub(super) fn handle_model_selection_command(&mut self, search: Option<&str>) -> Result<()> {
        let search_term = search.unwrap_or_default().trim();
        if let Some((model, thinking)) = self.find_exact_model_match(search_term) {
            self.apply_model_selection(model, thinking);
            return Ok(());
        }
        if let Some((model, thinking)) = self.find_unique_model_match(search_term) {
            self.apply_model_selection(model, thinking);
            return Ok(());
        }

        let normalized = self.normalize_model_selection(search_term);
        let needle = normalized.match_term.to_ascii_lowercase();
        let mut items: Vec<SelectItem> = self
            .get_model_candidates()
            .into_iter()
            .filter(|model| {
                if let Some(provider) = normalized.provider_filter.as_deref()
                    && model.provider != provider
                {
                    return false;
                }
                if needle.is_empty() {
                    true
                } else {
                    let provider_id =
                        format!("{}/{}", model.provider, model.id).to_ascii_lowercase();
                    let provider_colon_id =
                        format!("{}:{}", model.provider, model.id).to_ascii_lowercase();
                    provider_id.contains(&needle)
                        || provider_colon_id.contains(&needle)
                        || model.id.to_ascii_lowercase().contains(&needle)
                        || model.name.to_ascii_lowercase().contains(&needle)
                }
            })
            .map(|model| SelectItem {
                label: format!("{}/{}", model.provider, model.id),
                detail: Some(model.name.clone()),
                value: format!("{}/{}", model.provider, model.id),
            })
            .collect();
        items.sort_by(|a, b| a.label.cmp(&b.label));

        self.send_command(TuiCommand::OpenSelectMenu {
            menu_id: "model".to_string(),
            title: if search_term.is_empty() {
                "Select model".to_string()
            } else {
                format!("Select model matching '{search_term}'")
            },
            items,
            selected_value: None,
        });
        Ok(())
    }

    pub(crate) fn maybe_switch_to_preferred_post_login_model(
        &mut self,
        provider: &str,
    ) -> Option<String> {
        let settings = Settings::load_merged(&self.session_setup.tool_ctx.cwd);
        let preferred_provider = match provider {
            "openai-codex" => "openai",
            other => other,
        };
        let preferred_model_id = if settings.default_provider.as_deref() == Some(preferred_provider)
            || (provider == "openai-codex"
                && settings.default_provider.as_deref() == Some("openai-codex"))
        {
            crate::login::available_model_for_provider(
                &settings,
                preferred_provider,
                settings.default_model.as_deref(),
            )?
        } else {
            crate::login::preferred_available_model_for_provider(&settings, preferred_provider)?
        };
        let mut registry = ModelRegistry::new();
        registry.load_custom_models(&settings);
        crate::login::add_cached_github_copilot_models(&mut registry);
        let model = registry
            .find(preferred_provider, &preferred_model_id)
            .cloned()
            .or_else(|| {
                registry
                    .find_fuzzy(&preferred_model_id, Some(preferred_provider))
                    .cloned()
            })?;
        let display = format!("{}/{}", model.provider, model.id);
        self.apply_model_selection(model, None);
        Some(display)
    }

    pub(super) fn apply_model_selection(
        &mut self,
        model: Model,
        thinking_override: Option<ThinkingLevel>,
    ) {
        let api_key = crate::login::resolve_api_key(&model.provider).unwrap_or_default();
        let base_url = if model.provider == "github-copilot" {
            crate::login::github_copilot_api_base_url()
        } else {
            model.base_url.clone().unwrap_or_else(|| match model.api {
                ApiType::AnthropicMessages => "https://api.anthropic.com".to_string(),
                ApiType::GoogleGenerative => {
                    "https://generativelanguage.googleapis.com".to_string()
                }
                _ => "https://api.openai.com/v1".to_string(),
            })
        };
        let headers = if model.provider == "github-copilot" {
            crate::login::github_copilot_runtime_headers()
        } else {
            std::collections::HashMap::new()
        };
        let new_provider: std::sync::Arc<dyn bb_provider::Provider> = match model.api {
            ApiType::AnthropicMessages => std::sync::Arc::new(AnthropicProvider::new()),
            ApiType::GoogleGenerative => std::sync::Arc::new(GoogleProvider::new()),
            _ => std::sync::Arc::new(OpenAiProvider::new()),
        };
        let display = format!("{}/{}", model.provider, model.id);

        self.runtime_host.session_mut().set_model(ModelRef {
            provider: model.provider.clone(),
            id: model.id.clone(),
            reasoning: model.reasoning,
        });
        if let Some(level) = thinking_override {
            self.session_setup.thinking_level = level.as_str().to_string();
            self.runtime_host.session_mut().set_thinking_level(level);
        }
        self.runtime_host
            .runtime_mut()
            .set_model(Some(RuntimeModelRef {
                provider: model.provider.clone(),
                id: model.id.clone(),
                context_window: model.context_window as usize,
            }));
        self.session_setup.model = model;
        self.session_setup.provider = new_provider;
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
        let status = if let Some(level) = thinking_override {
            format!("Model: {display} • thinking: {}", level.as_str())
        } else {
            format!("Model: {display}")
        };
        self.options.model_display = Some(display);
        self.publish_footer();
        self.send_command(TuiCommand::SetStatusLine(status));
    }

    pub(super) fn get_model_candidates(&self) -> Vec<Model> {
        let settings = Settings::load_merged(&self.session_setup.tool_ctx.cwd);
        crate::login::authenticated_model_candidates(&settings)
    }

    pub(super) fn find_exact_model_match(
        &self,
        search_term: &str,
    ) -> Option<(Model, Option<ThinkingLevel>)> {
        let normalized = self.normalize_model_selection(search_term);
        let needle = normalized.match_term.to_ascii_lowercase();
        self.get_model_candidates().into_iter().find_map(|model| {
            if let Some(provider) = normalized.provider_filter.as_deref()
                && model.provider != provider
            {
                return None;
            }
            let provider_id = format!("{}/{}", model.provider, model.id).to_ascii_lowercase();
            let provider_colon_id = format!("{}:{}", model.provider, model.id).to_ascii_lowercase();
            let matched = model.id.eq_ignore_ascii_case(&needle)
                || model.name.eq_ignore_ascii_case(&needle)
                || provider_id == needle
                || provider_colon_id == needle;
            matched.then_some((model, normalized.thinking_override))
        })
    }

    pub(super) fn find_unique_model_match(
        &self,
        search_term: &str,
    ) -> Option<(Model, Option<ThinkingLevel>)> {
        let normalized = self.normalize_model_selection(search_term);
        if normalized.match_term.is_empty() {
            return None;
        }
        let needle = normalized.match_term.to_ascii_lowercase();
        let mut matches = self.get_model_candidates().into_iter().filter(|model| {
            if let Some(provider) = normalized.provider_filter.as_deref()
                && model.provider != provider
            {
                return false;
            }
            let provider_id = format!("{}/{}", model.provider, model.id).to_ascii_lowercase();
            let provider_colon_id = format!("{}:{}", model.provider, model.id).to_ascii_lowercase();
            provider_id.contains(&needle)
                || provider_colon_id.contains(&needle)
                || model.id.to_ascii_lowercase().contains(&needle)
                || model.name.to_ascii_lowercase().contains(&needle)
        });
        let first = matches.next()?;
        if matches.next().is_some() {
            return None;
        }
        Some((first, normalized.thinking_override))
    }

    fn normalize_model_selection(&self, search_term: &str) -> NormalizedModelSelection {
        let search_term = search_term.trim();
        if search_term.is_empty() {
            return NormalizedModelSelection::default();
        }

        let current_provider = self.session_setup.model.provider.as_str();
        let (parsed_provider, parsed_model, thinking_override) =
            parse_model_arg(Some(current_provider), Some(search_term));
        let thinking_override = thinking_override.as_deref().and_then(ThinkingLevel::parse);

        if search_term.contains('/') {
            return NormalizedModelSelection {
                provider_filter: Some(parsed_provider),
                match_term: parsed_model,
                thinking_override,
            };
        }

        if let Some((provider, model)) = search_term.split_once(':')
            && !provider.is_empty()
            && !model.is_empty()
            && self
                .get_model_candidates()
                .iter()
                .any(|candidate| candidate.provider.eq_ignore_ascii_case(provider))
        {
            return NormalizedModelSelection {
                provider_filter: Some(provider.to_string()),
                match_term: model.to_string(),
                thinking_override,
            };
        }

        NormalizedModelSelection {
            provider_filter: None,
            match_term: if parsed_provider == current_provider {
                parsed_model
            } else {
                search_term.to_string()
            },
            thinking_override,
        }
    }

    pub(super) fn copy_last_assistant_message(&mut self) -> Result<()> {
        let session_context =
            context::build_context(&self.session_setup.conn, &self.session_setup.session_id)?;
        let last_text =
            session_context
                .messages
                .into_iter()
                .rev()
                .find_map(|message| match message {
                    AgentMessage::Assistant(message) => {
                        let text = format_assistant_text(&message);
                        if text.trim().is_empty() {
                            None
                        } else {
                            Some(text)
                        }
                    }
                    _ => None,
                });

        if let Some(text) = last_text {
            copy_text_to_clipboard(&text)?;
            self.send_command(TuiCommand::SetStatusLine(
                "Copied last assistant message to clipboard".to_string(),
            ));
        } else {
            self.send_command(TuiCommand::SetStatusLine(
                "No assistant messages to copy".to_string(),
            ));
        }
        Ok(())
    }
}
