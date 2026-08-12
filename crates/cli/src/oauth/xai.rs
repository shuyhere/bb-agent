//! xAI SuperGrok / X Premium+ OAuth (device-code flow).
//!
//! Uses the public Grok CLI OIDC client (`auth method: none`) that xAI exposes
//! for SuperGrok / Premium+ subscription access. BB-Agent does not ship a
//! client secret. Entitlement is enforced by xAI; some accounts may receive
//! HTTP 403 even with an active web subscription.

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::json;
use tokio::time::{Duration, sleep};

use super::{OAuthCallbacks, OAuthCredentials, OAuthDeviceCode};

/// Public Grok CLI / SuperGrok OAuth client (OIDC public client).
/// Same client_id used by the Grok CLI / Hermes SuperGrok OAuth path.
const CLIENT_ID: &str = "b1a00492-073a-47ea-816f-4c329264a828";
const ISSUER: &str = "https://auth.x.ai";
const DISCOVERY_URL: &str = "https://auth.x.ai/.well-known/openid-configuration";
const DEVICE_CODE_URL: &str = "https://auth.x.ai/oauth2/device/code";
const DEFAULT_TOKEN_URL: &str = "https://auth.x.ai/oauth2/token";
const SCOPE: &str = "openid profile email offline_access grok-cli:access api:access";
pub(crate) const DEFAULT_INFERENCE_BASE_URL: &str = "https://api.x.ai/v1";

#[derive(Debug, Clone, Deserialize)]
struct OidcDiscovery {
    authorization_endpoint: String,
    token_endpoint: String,
    #[serde(default)]
    device_authorization_endpoint: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct DeviceCodeResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    #[serde(default)]
    verification_uri_complete: Option<String>,
    expires_in: i64,
    #[serde(default)]
    interval: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    refresh_token: Option<String>,
    #[serde(default)]
    id_token: Option<String>,
    #[serde(default)]
    expires_in: Option<i64>,
    #[serde(default)]
    token_type: Option<String>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    error_description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DevicePollOutcome {
    Pending,
    SlowDown,
    Fatal(String),
}

/// Validate that an OAuth endpoint is HTTPS and belongs to xAI auth hosts.
pub(crate) fn validate_oauth_endpoint(url: &str, field: &str) -> Result<String> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        bail!("xAI OIDC discovery missing {field}");
    }
    let parsed = url::Url::parse(trimmed).with_context(|| format!("Invalid {field}: {trimmed}"))?;
    if parsed.scheme() != "https" {
        bail!("Refusing non-HTTPS xAI {field}: {trimmed}");
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("xAI {field} has no host: {trimmed}"))?
        .to_ascii_lowercase();
    let allowed =
        host == "auth.x.ai" || host == "accounts.x.ai" || host.ends_with(".x.ai") || host == "x.ai";
    if !allowed {
        bail!("Refusing non-xAI {field} host `{host}` (expected *.x.ai)");
    }
    Ok(trimmed.to_string())
}

pub(crate) fn validate_inference_base_url(url: &str) -> Result<String> {
    let trimmed = url.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return Ok(DEFAULT_INFERENCE_BASE_URL.to_string());
    }
    let parsed =
        url::Url::parse(trimmed).with_context(|| format!("Invalid xAI base URL: {trimmed}"))?;
    if parsed.scheme() != "https" {
        bail!("Refusing non-HTTPS xAI base URL: {trimmed}");
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("xAI base URL has no host"))?
        .to_ascii_lowercase();
    if host != "api.x.ai" && !host.ends_with(".x.ai") && host != "x.ai" {
        bail!("Refusing non-xAI inference host `{host}`");
    }
    Ok(trimmed.to_string())
}

/// Classify a non-200 device-code token poll response body.
pub(crate) fn classify_device_poll_error(status: u16, body: &str) -> DevicePollOutcome {
    if let Ok(payload) = serde_json::from_str::<TokenResponse>(body) {
        match payload.error.as_deref() {
            Some("authorization_pending") => return DevicePollOutcome::Pending,
            Some("slow_down") => return DevicePollOutcome::SlowDown,
            Some(code) => {
                let detail = payload
                    .error_description
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| code.to_string());
                return DevicePollOutcome::Fatal(format!("xAI device-code poll failed: {detail}"));
            }
            None => {}
        }
    }
    DevicePollOutcome::Fatal(format!(
        "xAI device-code token polling failed (HTTP {status}): {}",
        body.trim()
    ))
}

pub(crate) fn format_tier_denied_error(context: &str, detail: &str) -> String {
    let mut message = format!(
        "xAI OAuth {context} was denied (HTTP 403). This account may not be entitled for SuperGrok API/OAuth access even if a web subscription is active."
    );
    if !detail.trim().is_empty() {
        message.push(' ');
        message.push_str(detail.trim());
    }
    message.push_str(
        " Re-login usually will not fix this. Use `bb login xai` with an API key (`XAI_API_KEY`) or check your plan at https://x.ai/grok.",
    );
    message
}

fn expires_at_ms(expires_in: Option<i64>) -> i64 {
    let now_ms = chrono::Utc::now().timestamp_millis();
    match expires_in {
        Some(seconds) if seconds > 0 => now_ms + seconds * 1000,
        _ => now_ms + 3600 * 1000,
    }
}

fn credentials_from_token_response(
    token: TokenResponse,
    token_endpoint: &str,
    fallback_refresh: Option<&str>,
) -> Result<OAuthCredentials> {
    let access = token
        .access_token
        .filter(|s| !s.trim().is_empty())
        .context("xAI token response missing access_token")?;
    let refresh = token
        .refresh_token
        .filter(|s| !s.trim().is_empty())
        .or_else(|| fallback_refresh.map(str::to_string))
        .filter(|s| !s.trim().is_empty())
        .context("xAI token response missing refresh_token")?;
    let expires = expires_at_ms(token.expires_in);

    let inference_base_url = validate_inference_base_url(
        std::env::var("XAI_BASE_URL")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .as_deref()
            .unwrap_or(DEFAULT_INFERENCE_BASE_URL),
    )?;

    Ok(OAuthCredentials {
        access,
        refresh,
        expires,
        extra: json!({
            "token_endpoint": token_endpoint,
            "issuer": ISSUER,
            "client_id": CLIENT_ID,
            "token_type": token.token_type.unwrap_or_else(|| "Bearer".to_string()),
            "id_token": token.id_token.unwrap_or_default(),
            "inference_base_url": inference_base_url,
            "auth_mode": "oauth_device_code",
        }),
    })
}

async fn discover_oidc(client: &reqwest::Client) -> Result<OidcDiscovery> {
    let resp = client
        .get(DISCOVERY_URL)
        .header("Accept", "application/json")
        .send()
        .await
        .context("xAI OIDC discovery request failed")?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        bail!("xAI OIDC discovery failed ({status}): {body}");
    }
    let discovery: OidcDiscovery = resp
        .json()
        .await
        .context("xAI OIDC discovery returned invalid JSON")?;
    let authorization_endpoint =
        validate_oauth_endpoint(&discovery.authorization_endpoint, "authorization_endpoint")?;
    let token_endpoint = validate_oauth_endpoint(&discovery.token_endpoint, "token_endpoint")?;
    let device_authorization_endpoint = match discovery.device_authorization_endpoint {
        Some(url) => Some(validate_oauth_endpoint(
            &url,
            "device_authorization_endpoint",
        )?),
        None => None,
    };
    Ok(OidcDiscovery {
        authorization_endpoint,
        token_endpoint,
        device_authorization_endpoint,
    })
}

async fn request_device_code(
    client: &reqwest::Client,
    device_url: &str,
) -> Result<DeviceCodeResponse> {
    let resp = client
        .post(device_url)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("Accept", "application/json")
        .form(&[("client_id", CLIENT_ID), ("scope", SCOPE)])
        .send()
        .await
        .context("xAI device-code request failed")?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        bail!("xAI device-code request failed ({status}): {body}");
    }
    let payload: DeviceCodeResponse = resp
        .json()
        .await
        .context("xAI device-code response was not valid JSON")?;
    if payload.device_code.trim().is_empty()
        || payload.user_code.trim().is_empty()
        || payload.verification_uri.trim().is_empty()
    {
        bail!("xAI device-code response missing required fields");
    }
    Ok(payload)
}

async fn poll_device_token(
    client: &reqwest::Client,
    token_endpoint: &str,
    device: &DeviceCodeResponse,
    on_progress: &Option<Box<dyn Fn(String) + Send>>,
) -> Result<TokenResponse> {
    let deadline =
        tokio::time::Instant::now() + Duration::from_secs(device.expires_in.max(1) as u64);
    let mut interval = Duration::from_secs(device.interval.unwrap_or(5).max(1));

    loop {
        if tokio::time::Instant::now() >= deadline {
            bail!("Timed out waiting for xAI device authorization. Run login again.");
        }

        let resp = client
            .post(token_endpoint)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .header("Accept", "application/json")
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ("client_id", CLIENT_ID),
                ("device_code", device.device_code.as_str()),
            ])
            .send()
            .await
            .context("xAI device-code token poll request failed")?;

        let status = resp.status();
        if status.is_success() {
            let token: TokenResponse = resp
                .json()
                .await
                .context("xAI device-code token response was not valid JSON")?;
            if token
                .access_token
                .as_ref()
                .is_none_or(|s| s.trim().is_empty())
            {
                bail!("xAI device-code token response missing access_token");
            }
            if token
                .refresh_token
                .as_ref()
                .is_none_or(|s| s.trim().is_empty())
            {
                bail!("xAI device-code token response missing refresh_token");
            }
            return Ok(token);
        }

        let body = resp.text().await.unwrap_or_default();
        match classify_device_poll_error(status.as_u16(), &body) {
            DevicePollOutcome::Pending => {
                if let Some(on_progress) = on_progress {
                    on_progress("Waiting for xAI device authorization…".into());
                }
                sleep(interval).await;
            }
            DevicePollOutcome::SlowDown => {
                interval = (interval + Duration::from_secs(1)).min(Duration::from_secs(30));
                if let Some(on_progress) = on_progress {
                    on_progress(format!(
                        "xAI asked to slow down; polling every {}s…",
                        interval.as_secs()
                    ));
                }
                sleep(interval).await;
            }
            DevicePollOutcome::Fatal(message) => bail!(message),
        }
    }
}

/// Run the full xAI SuperGrok device-code OAuth login flow.
pub async fn login_xai(callbacks: OAuthCallbacks) -> Result<OAuthCredentials> {
    let client = reqwest::Client::new();

    if let Some(ref on_progress) = callbacks.on_progress {
        on_progress("Discovering xAI OAuth endpoints…".into());
    }
    let discovery = discover_oidc(&client).await?;
    let device_url = discovery
        .device_authorization_endpoint
        .clone()
        .unwrap_or_else(|| DEVICE_CODE_URL.to_string());
    let device_url = validate_oauth_endpoint(&device_url, "device_authorization_endpoint")?;

    if let Some(ref on_progress) = callbacks.on_progress {
        on_progress("Requesting xAI device code…".into());
    }
    let device = request_device_code(&client, &device_url).await?;
    let verification_url = device
        .verification_uri_complete
        .clone()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| device.verification_uri.clone());

    (callbacks.on_auth)(verification_url.clone());
    if let Some(ref on_device_code) = callbacks.on_device_code {
        on_device_code(OAuthDeviceCode {
            user_code: device.user_code.clone(),
            verification_uri: verification_url,
        });
    }
    if let Some(ref on_progress) = callbacks.on_progress {
        on_progress(format!(
            "Open the verification URL and enter code {} if prompted",
            device.user_code
        ));
    }

    let token = poll_device_token(
        &client,
        &discovery.token_endpoint,
        &device,
        &callbacks.on_progress,
    )
    .await?;

    if let Some(ref on_progress) = callbacks.on_progress {
        on_progress("Validating xAI OAuth access…".into());
    }

    let creds = credentials_from_token_response(token, &discovery.token_endpoint, None)?;
    // Best-effort entitlement probe; ignore network blips but surface hard 403.
    if let Err(err) = probe_models_access(&client, &creds.access).await {
        let message = err.to_string();
        if message.contains("HTTP 403") {
            bail!(format_tier_denied_error("access probe", &message));
        }
        if let Some(ref on_progress) = callbacks.on_progress {
            on_progress(format!(
                "Login saved, but model probe was inconclusive: {message}"
            ));
        }
    }

    Ok(creds)
}

async fn probe_models_access(client: &reqwest::Client, access_token: &str) -> Result<()> {
    let url = format!("{DEFAULT_INFERENCE_BASE_URL}/models");
    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {access_token}"))
        .header("Accept", "application/json")
        .send()
        .await
        .context("xAI /models probe failed")?;
    let status = resp.status();
    if status.as_u16() == 403 {
        let body = resp.text().await.unwrap_or_default();
        bail!("HTTP 403: {body}");
    }
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        bail!("HTTP {status}: {body}");
    }
    Ok(())
}

/// Refresh an existing xAI OAuth access token.
pub async fn refresh_xai_token(
    refresh_token: &str,
    token_endpoint: Option<&str>,
) -> Result<OAuthCredentials> {
    if refresh_token.trim().is_empty() {
        bail!("xAI OAuth is missing refresh_token. Run `bb login xai` and choose SuperGrok OAuth.");
    }

    let client = reqwest::Client::new();
    let endpoint = if let Some(url) = token_endpoint.map(str::trim).filter(|s| !s.is_empty()) {
        validate_oauth_endpoint(url, "token_endpoint")?
    } else {
        match discover_oidc(&client).await {
            Ok(discovery) => discovery.token_endpoint,
            Err(_) => validate_oauth_endpoint(DEFAULT_TOKEN_URL, "token_endpoint")?,
        }
    };

    let resp = client
        .post(&endpoint)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("Accept", "application/json")
        .form(&[
            ("grant_type", "refresh_token"),
            ("client_id", CLIENT_ID),
            ("refresh_token", refresh_token),
        ])
        .send()
        .await
        .context("xAI token refresh request failed")?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        if status.as_u16() == 403 {
            bail!(format_tier_denied_error("token refresh", &body));
        }
        if status.as_u16() == 400 || status.as_u16() == 401 {
            bail!(
                "xAI token refresh failed ({status}): {body}. Re-authenticate with `bb login xai` (SuperGrok OAuth)."
            );
        }
        bail!("xAI token refresh failed ({status}): {body}");
    }

    let token: TokenResponse = resp
        .json()
        .await
        .context("xAI token refresh returned invalid JSON")?;
    credentials_from_token_response(token, &endpoint, Some(refresh_token))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_oauth_endpoint_accepts_auth_x_ai() {
        let url = validate_oauth_endpoint("https://auth.x.ai/oauth2/token", "token_endpoint")
            .expect("valid");
        assert_eq!(url, "https://auth.x.ai/oauth2/token");
    }

    #[test]
    fn validate_oauth_endpoint_rejects_http_and_foreign_hosts() {
        assert!(
            validate_oauth_endpoint("http://auth.x.ai/oauth2/token", "token_endpoint").is_err()
        );
        assert!(
            validate_oauth_endpoint("https://evil.example/oauth2/token", "token_endpoint").is_err()
        );
    }

    #[test]
    fn validate_inference_base_url_defaults_and_checks_host() {
        assert_eq!(
            validate_inference_base_url("").unwrap(),
            DEFAULT_INFERENCE_BASE_URL
        );
        assert_eq!(
            validate_inference_base_url("https://api.x.ai/v1/").unwrap(),
            "https://api.x.ai/v1"
        );
        assert!(validate_inference_base_url("https://api.openai.com/v1").is_err());
    }

    #[test]
    fn classify_device_poll_error_handles_pending_and_slow_down() {
        assert_eq!(
            classify_device_poll_error(400, r#"{"error":"authorization_pending"}"#),
            DevicePollOutcome::Pending
        );
        assert_eq!(
            classify_device_poll_error(400, r#"{"error":"slow_down"}"#),
            DevicePollOutcome::SlowDown
        );
        match classify_device_poll_error(
            400,
            r#"{"error":"access_denied","error_description":"nope"}"#,
        ) {
            DevicePollOutcome::Fatal(msg) => assert!(msg.contains("nope")),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn format_tier_denied_error_mentions_api_key_fallback() {
        let msg = format_tier_denied_error("token refresh", "forbidden");
        assert!(msg.contains("HTTP 403"));
        assert!(msg.contains("XAI_API_KEY"));
        assert!(msg.contains("forbidden"));
    }

    #[test]
    fn credentials_from_token_response_requires_tokens() {
        let ok = credentials_from_token_response(
            TokenResponse {
                access_token: Some("at".into()),
                refresh_token: Some("rt".into()),
                id_token: None,
                expires_in: Some(120),
                token_type: Some("Bearer".into()),
                error: None,
                error_description: None,
            },
            DEFAULT_TOKEN_URL,
            None,
        )
        .expect("creds");
        assert_eq!(ok.access, "at");
        assert_eq!(ok.refresh, "rt");
        assert!(ok.expires > chrono::Utc::now().timestamp_millis());
        assert_eq!(
            ok.extra.get("token_endpoint").and_then(|v| v.as_str()),
            Some(DEFAULT_TOKEN_URL)
        );

        assert!(
            credentials_from_token_response(
                TokenResponse {
                    access_token: Some("at".into()),
                    refresh_token: None,
                    id_token: None,
                    expires_in: None,
                    token_type: None,
                    error: None,
                    error_description: None,
                },
                DEFAULT_TOKEN_URL,
                None,
            )
            .is_err()
        );
    }
}
