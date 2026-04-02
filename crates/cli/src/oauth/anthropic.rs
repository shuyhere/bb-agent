use anyhow::{Context, Result};
use serde::Deserialize;

use super::callback_server::{start_callback_server, CallbackParams, CallbackServerParts};
use super::pkce::generate_pkce;
use super::{OAuthCallbacks, OAuthCredentials};

// ── Constants (matching pi) ──────────────────────────────────────────

const CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
const AUTHORIZE_URL: &str = "https://claude.ai/oauth/authorize";
const TOKEN_URL: &str = "https://platform.claude.com/v1/oauth/token";
const REDIRECT_URI: &str = "http://localhost:53692/callback";
const CALLBACK_PORT: u16 = 53692;
const CALLBACK_PATH: &str = "/callback";
const SCOPES: &str = "org:create_api_key user:profile user:inference user:sessions:claude_code user:mcp_servers user:file_upload";

// ── Token response ──────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: String,
    #[serde(default)]
    expires_in: i64,
}

// ── Public API ──────────────────────────────────────────────────────

/// Run the full Anthropic OAuth authorization-code + PKCE flow.
///
/// The caller provides `OAuthCallbacks` so this function stays UI-agnostic.
pub async fn login_anthropic(callbacks: OAuthCallbacks) -> Result<OAuthCredentials> {
    let (verifier, challenge) = generate_pkce();

    // Pi uses the PKCE verifier as the state parameter for Anthropic.
    let state = verifier.clone();

    // Build auth URL — must match pi exactly.
    let auth_url = format!(
        "{AUTHORIZE_URL}?\
         code=true\
         &client_id={CLIENT_ID}\
         &response_type=code\
         &redirect_uri={redirect}\
         &scope={scopes}\
         &code_challenge={challenge}\
         &code_challenge_method=S256\
         &state={state}",
        redirect = url_encode(REDIRECT_URI),
        scopes = url_encode(SCOPES),
    );

    // Tell the UI about the URL.
    (callbacks.on_auth)(auth_url);

    if let Some(ref on_progress) = callbacks.on_progress {
        on_progress("Waiting for browser authentication…".into());
    }

    // Start local callback server.
    let server = start_callback_server(CALLBACK_PORT, CALLBACK_PATH).await?;

    // Race: browser callback vs manual paste.
    let CallbackServerParts { result_rx, cancel_tx } = server.into_parts();
    let params = match callbacks.on_manual_input {
        Some(manual_rx) => {
            tokio::select! {
                result = result_rx => {
                    result.map_err(|_| anyhow::anyhow!("Callback channel closed"))??
                }
                manual = manual_rx => {
                    let _ = cancel_tx.send(());
                    let raw = manual.map_err(|_| anyhow::anyhow!("Manual input cancelled"))?;
                    let parsed = parse_authorization_input(&raw);
                    let code = parsed.code.ok_or_else(|| anyhow::anyhow!("No authorization code found in pasted input"))?;
                    let parsed_state = parsed.state.unwrap_or_else(|| state.clone());
                    CallbackParams { code, state: parsed_state }
                }
            }
        }
        None => result_rx
            .await
            .map_err(|_| anyhow::anyhow!("Callback channel closed"))??,
    };

    if let Some(ref on_progress) = callbacks.on_progress {
        on_progress("Exchanging authorization code for tokens…".into());
    }

    exchange_code(&params.code, &params.state, &verifier).await
}

/// Refresh an existing Anthropic OAuth token.
pub async fn refresh_anthropic_token(refresh_token: &str) -> Result<OAuthCredentials> {
    // Anthropic uses JSON body for token requests — must match pi.
    let client = reqwest::Client::new();
    let resp = client
        .post(TOKEN_URL)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .json(&serde_json::json!({
            "grant_type": "refresh_token",
            "client_id": CLIENT_ID,
            "refresh_token": refresh_token,
        }))
        .send()
        .await
        .context("Failed to send refresh request to Anthropic")?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("Anthropic token refresh failed ({status}): {body}");
    }

    let token: TokenResponse = resp.json().await.context("Failed to parse token response")?;
    let now_ms = chrono::Utc::now().timestamp_millis();

    Ok(OAuthCredentials {
        access: token.access_token,
        refresh: token.refresh_token,
        expires: now_ms + token.expires_in * 1000,
        extra: serde_json::Value::Null,
    })
}

// ── Internals ───────────────────────────────────────────────────────

async fn exchange_code(code: &str, state: &str, verifier: &str) -> Result<OAuthCredentials> {
    // Anthropic uses JSON body (not form-encoded) — must match pi exactly.
    let client = reqwest::Client::new();
    let resp = client
        .post(TOKEN_URL)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .json(&serde_json::json!({
            "grant_type": "authorization_code",
            "client_id": CLIENT_ID,
            "code": code,
            "state": state,
            "redirect_uri": REDIRECT_URI,
            "code_verifier": verifier,
        }))
        .send()
        .await
        .context("Failed to send token exchange request to Anthropic")?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("Anthropic token exchange failed ({status}): {body}");
    }

    let token: TokenResponse = resp.json().await.context("Failed to parse token response")?;
    let now_ms = chrono::Utc::now().timestamp_millis();

    Ok(OAuthCredentials {
        access: token.access_token,
        refresh: token.refresh_token,
        expires: now_ms + token.expires_in * 1000,
        extra: serde_json::Value::Null,
    })
}

// ── Input parsing (matches pi's parseAuthorizationInput) ────────────

struct ParsedInput {
    code: Option<String>,
    state: Option<String>,
}

fn parse_authorization_input(input: &str) -> ParsedInput {
    let value = input.trim();
    if value.is_empty() {
        return ParsedInput { code: None, state: None };
    }
    if let Ok(url) = url::Url::parse(value) {
        let code = url.query_pairs().find(|(k, _)| k == "code").map(|(_, v)| v.to_string());
        let state = url.query_pairs().find(|(k, _)| k == "state").map(|(_, v)| v.to_string());
        if code.is_some() {
            return ParsedInput { code, state };
        }
    }
    if value.contains('#') {
        let parts: Vec<&str> = value.splitn(2, '#').collect();
        return ParsedInput {
            code: parts.first().map(|s| s.to_string()),
            state: parts.get(1).map(|s| s.to_string()),
        };
    }
    if value.contains("code=") {
        let pairs: std::collections::HashMap<String, String> = value
            .split('&')
            .filter_map(|p| p.split_once('='))
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        return ParsedInput {
            code: pairs.get("code").cloned(),
            state: pairs.get("state").cloned(),
        };
    }
    ParsedInput {
        code: Some(value.to_string()),
        state: None,
    }
}

/// Minimal percent-encoding for URL query values.
fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 2);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push('%');
                out.push_str(&format!("{b:02X}"));
            }
        }
    }
    out
}
