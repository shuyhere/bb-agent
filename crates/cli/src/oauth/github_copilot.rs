use anyhow::Result;

use super::{OAuthCallbacks, OAuthCredentials, OAuthDeviceCode};

fn verification_uri_for_authority(authority: &str) -> String {
    if authority.eq_ignore_ascii_case("github.com") {
        "https://github.com/login/device".to_string()
    } else {
        format!("https://{authority}/login/device")
    }
}

pub async fn login_github_copilot(
    authority: &str,
    callbacks: OAuthCallbacks,
) -> Result<OAuthCredentials> {
    let verification_uri = verification_uri_for_authority(authority);

    (callbacks.on_auth)(verification_uri.clone());

    if let Some(ref on_progress) = callbacks.on_progress {
        on_progress(format!(
            "GitHub Copilot authority selected: {authority}. Device/browser auth scaffolding is ready, but token exchange is not implemented yet."
        ));
    }

    if let Some(ref on_device_code) = callbacks.on_device_code {
        on_device_code(OAuthDeviceCode {
            user_code: "pending-server-issued-code".to_string(),
            verification_uri: verification_uri.clone(),
        });
    }

    if let Some(manual_rx) = callbacks.on_manual_input {
        let _ = manual_rx.await;
    }

    anyhow::bail!(
        "GitHub Copilot OAuth/device flow is not implemented yet in bb; authority, dialog flow, and token plumbing skeleton are in place"
    )
}

pub async fn refresh_github_copilot_token(
    _refresh_token: &str,
    authority: &str,
) -> Result<OAuthCredentials> {
    anyhow::bail!(
        "GitHub Copilot token refresh is not implemented yet in bb for authority {authority}"
    )
}
