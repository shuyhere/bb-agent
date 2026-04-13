use super::dialogs::{
    build_api_key_dialog, build_copilot_enterprise_dialog, build_device_oauth_dialog,
    build_exchange_oauth_dialog, build_preparing_oauth_dialog, build_processing_oauth_dialog,
    build_waiting_oauth_dialog,
};
use super::*;

impl TuiController {
    pub(crate) async fn begin_oauth_login(
        &mut self,
        provider: &str,
        submission_rx: &mut tokio::sync::mpsc::UnboundedReceiver<
            bb_tui::tui::TuiSubmission,
        >,
    ) -> Result<()> {
        use crate::oauth::OAuthCallbacks;
        use bb_tui::tui::TuiSubmission;
        use std::sync::{Arc, Mutex};
        use tokio::sync::oneshot;

        let provider = crate::login::provider_oauth_variant(provider).unwrap_or(provider);
        let label = crate::login::provider_display_name(provider);
        let (manual_tx, manual_rx) = oneshot::channel::<String>();
        let mut manual_tx = Some(manual_tx);
        let dialog_shared = Arc::new(Mutex::new((None::<String>, None::<String>, None::<String>)));

        self.send_command(TuiCommand::SetLocalActionActive(true));
        self.send_command(TuiCommand::SetInput(String::new()));
        self.send_command(TuiCommand::OpenAuthDialog(
            build_preparing_oauth_dialog(&label, "Starting browser sign-in…"),
        ));
        self.send_command(TuiCommand::SetStatusLine(format!(
            "Starting OAuth login for {label}..."
        )));

        let command_tx = self.command_tx.clone();
        let label_for_auth = label.clone();
        let callbacks = OAuthCallbacks {
            on_auth: Box::new({
                let dialog_shared = dialog_shared.clone();
                move |url: String| {
                    let opened = crate::login::try_open_browser(&url);
                    let launcher_hint = if opened {
                        "A browser should open locally."
                    } else {
                        "No local browser launcher detected. Open the URL manually."
                    };
                    if let Ok(mut shared) = dialog_shared.lock() {
                        shared.0 = Some(url.clone());
                        shared.1 = Some(launcher_hint.to_string());
                        shared.2 = None;
                    }
                    let _ = command_tx.send(TuiCommand::UpdateAuthDialog(
                        build_waiting_oauth_dialog(
                            &label_for_auth,
                            "Waiting for browser authentication…",
                            Some(url),
                            Some(launcher_hint.to_string()),
                        ),
                    ));
                }
            }),
            on_device_code: Some(Box::new({
                let command_tx = self.command_tx.clone();
                let label = label.clone();
                let dialog_shared = dialog_shared.clone();
                move |device| {
                    if let Ok(mut shared) = dialog_shared.lock() {
                        shared.0 = Some(device.verification_uri.clone());
                        shared.1 =
                            Some("bb generated the device code shown on this screen.".to_string());
                        shared.2 = Some(device.user_code.clone());
                    }
                    let _ = command_tx.send(TuiCommand::SetStatusLine(format!(
                        "Enter device code {} from bb in your browser",
                        device.user_code
                    )));
                    let _ = command_tx.send(TuiCommand::UpdateAuthDialog(
                        build_device_oauth_dialog(
                            &label,
                            "Complete device authentication in your browser…",
                            device.verification_uri,
                            device.user_code,
                        ),
                    ));
                }
            })),
            on_manual_input: Some(manual_rx),
            on_progress: Some(Box::new({
                let command_tx = self.command_tx.clone();
                let label = label.clone();
                let dialog_shared = dialog_shared.clone();
                move |msg: String| {
                    let (url, launcher_hint, device_code) = if let Ok(shared) = dialog_shared.lock()
                    {
                        (shared.0.clone(), shared.1.clone(), shared.2.clone())
                    } else {
                        (None, None, None)
                    };
                    let _ = command_tx.send(TuiCommand::SetStatusLine(msg.clone()));
                    let dialog = if let Some(user_code) = device_code {
                        build_device_oauth_dialog(
                            &label,
                            &msg,
                            url.unwrap_or_else(|| "https://github.com/login/device".to_string()),
                            user_code,
                        )
                    } else if msg.contains("Exchanging authorization code") {
                        build_exchange_oauth_dialog(&label, &msg, url, launcher_hint)
                    } else {
                        build_waiting_oauth_dialog(&label, &msg, url, launcher_hint)
                    };
                    let _ = command_tx.send(TuiCommand::UpdateAuthDialog(dialog));
                }
            })),
        };

        let login = crate::login::run_oauth_login(provider, callbacks);
        tokio::pin!(login);

        let mut cancelled = false;
        let outcome = loop {
            tokio::select! {
                maybe_submission = submission_rx.recv() => {
                    match maybe_submission {
                        Some(TuiSubmission::CancelLocalAction) => {
                            cancelled = true;
                            if let Some(tx) = manual_tx.take() {
                                let _ = tx.send(String::new());
                            }
                            break Ok::<_, anyhow::Error>(());
                        }
                        Some(TuiSubmission::Input(text)) => {
                            let text = text.trim().to_string();
                            if !text.is_empty()
                                && let Some(tx) = manual_tx.take()
                            {
                                let _ = tx.send(text);
                                self.send_command(TuiCommand::SetInput(String::new()));
                                let (url, launcher_hint) = if let Ok(shared) = dialog_shared.lock() {
                                    (shared.0.clone(), shared.1.clone())
                                } else {
                                    (None, None)
                                };
                                self.send_command(TuiCommand::UpdateAuthDialog(
                                    build_processing_oauth_dialog(
                                        &label,
                                        "Processing pasted callback…",
                                        url,
                                        launcher_hint,
                                    ),
                                ));
                                self.send_command(TuiCommand::SetStatusLine(
                                    "Processing pasted callback...".to_string(),
                                ));
                            }
                        }
                        Some(TuiSubmission::InputWithImages { text, .. }) => {
                            let text = text.trim().to_string();
                            if !text.is_empty()
                                && let Some(tx) = manual_tx.take()
                            {
                                let _ = tx.send(text);
                                self.send_command(TuiCommand::SetInput(String::new()));
                                let (url, launcher_hint) = if let Ok(shared) = dialog_shared.lock() {
                                    (shared.0.clone(), shared.1.clone())
                                } else {
                                    (None, None)
                                };
                                self.send_command(TuiCommand::UpdateAuthDialog(
                                    build_processing_oauth_dialog(
                                        &label,
                                        "Processing pasted callback…",
                                        url,
                                        launcher_hint,
                                    ),
                                ));
                                self.send_command(TuiCommand::SetStatusLine(
                                    "Processing pasted callback...".to_string(),
                                ));
                            }
                        }
                        Some(TuiSubmission::MenuSelection { .. }) => {}
                        Some(TuiSubmission::ApprovalDecision { .. }) => {}
                        Some(TuiSubmission::EditQueuedMessages) => {}
                        None => {
                            cancelled = true;
                            if let Some(tx) = manual_tx.take() {
                                let _ = tx.send(String::new());
                            }
                            break Ok::<_, anyhow::Error>(());
                        }
                    }
                }
                result = &mut login => {
                    break result;
                }
            }
        };

        self.send_command(TuiCommand::SetLocalActionActive(false));
        match outcome {
            Ok(()) => {
                self.send_command(TuiCommand::CloseAuthDialog);
                if cancelled {
                    self.send_command(TuiCommand::SetStatusLine(
                        "Authentication cancelled".to_string(),
                    ));
                } else if provider == "github-copilot" {
                    let model_count = crate::login::github_copilot_cached_models().len();
                    if let Some(display) = self.maybe_switch_to_preferred_post_login_model(provider)
                    {
                        self.send_command(TuiCommand::SetStatusLine(format!(
                                "Logged in to {} • refreshed {} models • switched to {} • use /model to change",
                                crate::login::provider_display_name(provider),
                                model_count,
                                display,
                            )));
                    } else {
                        self.send_command(TuiCommand::SetStatusLine(format!(
                            "Logged in to {} • refreshed {} models • use /model to change",
                            crate::login::provider_display_name(provider),
                            model_count
                        )));
                    }
                } else if let Some(display) =
                    self.maybe_switch_to_preferred_post_login_model(provider)
                {
                    self.send_command(TuiCommand::SetStatusLine(format!(
                        "Logged in to {} • switched to {} • use /model to change",
                        crate::login::provider_display_name(provider),
                        display,
                    )));
                } else {
                    self.send_command(TuiCommand::SetStatusLine(format!(
                        "Logged in to {} • use /model to change",
                        crate::login::provider_display_name(provider)
                    )));
                }
                Ok(())
            }
            Err(err) => {
                self.send_command(TuiCommand::CloseAuthDialog);
                self.send_command(TuiCommand::PushNote {
                    level: TuiNoteLevel::Error,
                    text: format!("Authentication failed: {err}"),
                });
                self.send_command(TuiCommand::SetStatusLine(
                    "Authentication failed".to_string(),
                ));
                Ok(())
            }
        }
    }

    pub(crate) fn begin_api_key_login(&mut self, provider: &str) {
        let provider = crate::login::provider_api_key_variant(provider).unwrap_or(provider);
        self.pending_login_api_key_provider = Some(provider.to_string());
        let (_env_var, url) = crate::login::provider_meta(provider);
        let label = crate::login::provider_display_name(provider);
        self.send_command(TuiCommand::SetLocalActionActive(true));
        self.send_command(TuiCommand::SetInput(String::new()));
        self.send_command(TuiCommand::OpenAuthDialog(build_api_key_dialog(
            &label, url,
        )));
        self.send_command(TuiCommand::SetStatusLine(format!(
            "Paste API key for {label} and press Enter"
        )));
    }

    pub(crate) fn begin_copilot_enterprise_login(&mut self) {
        self.pending_login_copilot_enterprise = true;
        self.send_command(TuiCommand::SetLocalActionActive(true));
        self.send_command(TuiCommand::SetInput(String::new()));
        self.send_command(TuiCommand::OpenAuthDialog(
            build_copilot_enterprise_dialog(),
        ));
        self.send_command(TuiCommand::SetStatusLine(
            "Enter your GitHub Enterprise Server domain".to_string(),
        ));
    }

    pub(crate) fn finish_copilot_host_setup(&mut self, domain: &str) -> Result<()> {
        let domain = crate::login::normalize_github_domain(domain)?;
        crate::login::save_github_copilot_config(&domain)?;
        self.pending_login_copilot_enterprise = false;
        self.send_command(TuiCommand::CloseAuthDialog);
        self.send_command(TuiCommand::SetLocalActionActive(false));
        self.send_command(TuiCommand::SetInput(String::new()));
        self.send_command(TuiCommand::SetStatusLine(format!(
            "Saved GitHub Copilot host: {domain}"
        )));
        self.send_command(TuiCommand::PushNote {
            level: TuiNoteLevel::Status,
            text: format!(
                "GitHub Copilot authority configured for {domain}. bb will use this authority for the GitHub device flow and Copilot token exchange."
            ),
        });
        Ok(())
    }
}
