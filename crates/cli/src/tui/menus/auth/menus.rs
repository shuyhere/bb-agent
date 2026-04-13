use super::dialogs::{tui_auth_display_name, tui_auth_status_detail};
use super::*;

impl TuiController {
    pub(crate) fn open_login_provider_menu(&mut self) {
        self.send_command(TuiCommand::OpenSelectMenu {
            menu_id: LOGIN_PROVIDER_MENU_ID.to_string(),
            title: "Sign in provider".to_string(),
            items: LOGIN_PROVIDERS
                .iter()
                .map(|provider| {
                    let methods = match *provider {
                        "anthropic" | "openai" => "OAuth + API key",
                        "github-copilot" => "OAuth",
                        _ => "API key",
                    };
                    SelectItem {
                        label: match *provider {
                            "anthropic" => "Anthropic".to_string(),
                            "openai" => "OpenAI".to_string(),
                            "github-copilot" => "GitHub Copilot".to_string(),
                            "google" => "Google Gemini".to_string(),
                            "groq" => "Groq".to_string(),
                            "xai" => "xAI".to_string(),
                            "openrouter" => "OpenRouter".to_string(),
                            _ => (*provider).to_string(),
                        },
                        detail: Some(format!(
                            "{methods} • {}",
                            tui_auth_status_detail(provider)
                        )),
                        value: (*provider).to_string(),
                    }
                })
                .collect(),
            selected_value: None,
        });
    }

    pub(crate) fn open_login_method_menu(&mut self, provider: &str) {
        let mut items = Vec::new();
        match provider {
            "anthropic" => {
                items.push(SelectItem {
                    label: "Claude Pro/Max".to_string(),
                    detail: Some("OAuth subscription login".to_string()),
                    value: "oauth:anthropic".to_string(),
                });
                items.push(SelectItem {
                    label: "Anthropic API key".to_string(),
                    detail: Some("Use ANTHROPIC_API_KEY or paste a key".to_string()),
                    value: "api_key:anthropic".to_string(),
                });
            }
            "openai" => {
                items.push(SelectItem {
                    label: "ChatGPT Plus/Pro (Codex)".to_string(),
                    detail: Some("OAuth subscription login".to_string()),
                    value: "oauth:openai-codex".to_string(),
                });
                items.push(SelectItem {
                    label: "OpenAI API key".to_string(),
                    detail: Some("Use OPENAI_API_KEY or paste a key".to_string()),
                    value: "api_key:openai".to_string(),
                });
            }
            "github-copilot" => {
                items.push(SelectItem {
                    label: "GitHub.com".to_string(),
                    detail: Some("Use the default github.com Copilot authority".to_string()),
                    value: "copilot:github".to_string(),
                });
                items.push(SelectItem {
                    label: "GitHub Enterprise Server".to_string(),
                    detail: Some("Enter your GitHub Enterprise Server domain".to_string()),
                    value: "copilot:enterprise".to_string(),
                });
            }
            "google" => {
                items.push(SelectItem {
                    label: "Google API key".to_string(),
                    detail: Some("Use GOOGLE_API_KEY or paste a key".to_string()),
                    value: "api_key:google".to_string(),
                });
            }
            "groq" => {
                items.push(SelectItem {
                    label: "Groq API key".to_string(),
                    detail: Some("Use GROQ_API_KEY or paste a key".to_string()),
                    value: "api_key:groq".to_string(),
                });
            }
            "xai" => {
                items.push(SelectItem {
                    label: "xAI API key".to_string(),
                    detail: Some("Use XAI_API_KEY or paste a key".to_string()),
                    value: "api_key:xai".to_string(),
                });
            }
            "openrouter" => {
                items.push(SelectItem {
                    label: "OpenRouter API key".to_string(),
                    detail: Some("Use OPENROUTER_API_KEY or paste a key".to_string()),
                    value: "api_key:openrouter".to_string(),
                });
            }
            _ => {}
        }

        self.send_command(TuiCommand::OpenSelectMenu {
            menu_id: LOGIN_METHOD_MENU_ID.to_string(),
            title: format!(
                "Sign in method: {}",
                match provider {
                    "anthropic" => "Anthropic",
                    "openai" => "OpenAI",
                    "github-copilot" => "GitHub Copilot",
                    "google" => "Google Gemini",
                    "groq" => "Groq",
                    "xai" => "xAI",
                    "openrouter" => "OpenRouter",
                    _ => provider,
                }
            ),
            items,
            selected_value: None,
        });
    }

    pub(crate) fn open_logout_provider_menu(&mut self) {
        let providers = crate::login::configured_providers();
        if providers.is_empty() {
            self.send_command(TuiCommand::SetStatusLine(
                "No logged-in providers".to_string(),
            ));
            return;
        }
        self.send_command(TuiCommand::OpenSelectMenu {
            menu_id: LOGOUT_PROVIDER_MENU_ID.to_string(),
            title: "Logout provider".to_string(),
            items: providers
                .into_iter()
                .map(|provider| SelectItem {
                    label: tui_auth_display_name(&provider),
                    detail: Some(tui_auth_status_detail(&provider)),
                    value: provider,
                })
                .collect(),
            selected_value: None,
        });
    }
}
