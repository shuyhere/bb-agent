use super::*;

pub(super) fn tui_auth_display_name(provider: &str) -> String {
    crate::login::provider_display_name(provider).into_owned()
}

pub(super) fn tui_auth_status_detail(provider: &str) -> String {
    let base = crate::login::provider_auth_status_summary(provider);
    if provider == "github-copilot"
        && let Some(domain) = crate::login::github_copilot_domain()
    {
        return format!("{base} • host: {domain}");
    }
    base
}

enum OAuthDialogStage {
    Preparing,
    WaitingForBrowser,
    ProcessingCallback,
    ExchangingTokens,
}

fn auth_step(label: &str, state: TuiAuthStepState) -> TuiAuthStep {
    TuiAuthStep {
        label: label.to_string(),
        state: Some(state),
    }
}

fn build_oauth_dialog(
    label: &str,
    status: &str,
    stage: OAuthDialogStage,
    url: Option<String>,
    launcher_hint: Option<String>,
) -> TuiAuthDialog {
    let steps = match stage {
        OAuthDialogStage::Preparing => vec![
            auth_step("Open sign-in page", TuiAuthStepState::Active),
            auth_step(
                "Complete sign-in in your browser",
                TuiAuthStepState::Pending,
            ),
            auth_step(
                "Return via localhost callback or paste it here",
                TuiAuthStepState::Pending,
            ),
            auth_step("Save credentials", TuiAuthStepState::Pending),
        ],
        OAuthDialogStage::WaitingForBrowser => vec![
            auth_step("Open sign-in page", TuiAuthStepState::Done),
            auth_step("Complete sign-in in your browser", TuiAuthStepState::Active),
            auth_step(
                "Return via localhost callback or paste it here",
                TuiAuthStepState::Pending,
            ),
            auth_step("Save credentials", TuiAuthStepState::Pending),
        ],
        OAuthDialogStage::ProcessingCallback => vec![
            auth_step("Open sign-in page", TuiAuthStepState::Done),
            auth_step("Complete sign-in in your browser", TuiAuthStepState::Done),
            auth_step(
                "Return via localhost callback or paste it here",
                TuiAuthStepState::Active,
            ),
            auth_step("Save credentials", TuiAuthStepState::Pending),
        ],
        OAuthDialogStage::ExchangingTokens => vec![
            auth_step("Open sign-in page", TuiAuthStepState::Done),
            auth_step("Complete sign-in in your browser", TuiAuthStepState::Done),
            auth_step(
                "Return via localhost callback or paste it here",
                TuiAuthStepState::Done,
            ),
            auth_step("Save credentials", TuiAuthStepState::Active),
        ],
    };

    let mut lines = Vec::new();
    if let Some(hint) = launcher_hint {
        lines.push(hint);
    }
    if url.is_some() {
        lines.push(
            "If the browser opens on another machine, paste the full localhost callback URL below and press Enter."
                .to_string(),
        );
    } else {
        lines.push("The authorization URL will appear below.".to_string());
    }

    TuiAuthDialog {
        title: format!("Sign in to {label}"),
        status: Some(status.to_string()),
        steps,
        url,
        lines,
        input_label: Some("Localhost callback URL".to_string()),
        input_placeholder: Some("Paste full localhost callback URL here".to_string()),
    }
}

pub(super) fn build_processing_oauth_dialog(
    label: &str,
    status: &str,
    url: Option<String>,
    launcher_hint: Option<String>,
) -> TuiAuthDialog {
    build_oauth_dialog(
        label,
        status,
        OAuthDialogStage::ProcessingCallback,
        url,
        launcher_hint,
    )
}

pub(super) fn build_preparing_oauth_dialog(label: &str, status: &str) -> TuiAuthDialog {
    build_oauth_dialog(label, status, OAuthDialogStage::Preparing, None, None)
}

pub(super) fn build_waiting_oauth_dialog(
    label: &str,
    status: &str,
    url: Option<String>,
    launcher_hint: Option<String>,
) -> TuiAuthDialog {
    build_oauth_dialog(
        label,
        status,
        OAuthDialogStage::WaitingForBrowser,
        url,
        launcher_hint,
    )
}

pub(super) fn build_exchange_oauth_dialog(
    label: &str,
    status: &str,
    url: Option<String>,
    launcher_hint: Option<String>,
) -> TuiAuthDialog {
    build_oauth_dialog(
        label,
        status,
        OAuthDialogStage::ExchangingTokens,
        url,
        launcher_hint,
    )
}

pub(super) fn build_device_oauth_dialog(
    label: &str,
    status: &str,
    verification_uri: String,
    user_code: String,
) -> TuiAuthDialog {
    TuiAuthDialog {
        title: format!("Sign in to {label}"),
        status: Some(status.to_string()),
        steps: vec![
            auth_step("Open verification page", TuiAuthStepState::Done),
            auth_step(
                "Get the device code from this bb screen",
                TuiAuthStepState::Done,
            ),
            auth_step(
                "Enter the device code in your browser",
                TuiAuthStepState::Active,
            ),
            auth_step(
                "Wait for bb to finish sign-in automatically",
                TuiAuthStepState::Pending,
            ),
        ],
        url: Some(verification_uri),
        lines: vec![
            "bb generated the device code for you. Copy the code shown below from this screen and enter it on the GitHub verification page.".to_string(),
            format!("Device code (from bb): {user_code}"),
            "This flow does not use a localhost callback URL.".to_string(),
        ],
        input_label: None,
        input_placeholder: None,
    }
}

pub(super) fn build_copilot_enterprise_dialog() -> TuiAuthDialog {
    TuiAuthDialog {
        title: "GitHub Copilot Enterprise".to_string(),
        status: Some("Enter your GitHub Enterprise Server domain".to_string()),
        steps: vec![
            auth_step("Choose GitHub Enterprise Server host", TuiAuthStepState::Active),
            auth_step("Store host configuration", TuiAuthStepState::Pending),
            auth_step("Start Copilot OAuth/device flow", TuiAuthStepState::Pending),
        ],
        url: None,
        lines: vec![
            "Examples: github.acme.com or https://github.acme.com".to_string(),
            "Press Esc to cancel. Press Enter to save the host target, then bb will open the Copilot auth skeleton."
                .to_string(),
        ],
        input_label: Some("GitHub Enterprise Server domain".to_string()),
        input_placeholder: Some("github.example.com".to_string()),
    }
}

pub(super) fn build_api_key_dialog(label: &str, url: &str) -> TuiAuthDialog {
    TuiAuthDialog {
        title: format!("Sign in to {label}"),
        status: Some("Paste your API key to continue".to_string()),
        steps: vec![
            auth_step("Open API key page if needed", TuiAuthStepState::Done),
            auth_step("Paste API key", TuiAuthStepState::Active),
            auth_step("Save credentials", TuiAuthStepState::Pending),
        ],
        url: Some(url.to_string()),
        lines: vec!["Your input stays local and will be stored in auth.json.".to_string()],
        input_label: Some("API key".to_string()),
        input_placeholder: Some("Paste API key here".to_string()),
    }
}
