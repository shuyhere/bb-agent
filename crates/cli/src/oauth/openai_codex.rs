use anyhow::{Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::Deserialize;

use super::callback_server::{start_callback_server, CallbackParams, CallbackServerParts};
use super::pkce::generate_pkce;
use super::{OAuthCallbacks, OAuthCredentials};

// ── Constants (matching pi) ──────────────────────────────────────────

const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const AUTHORIZE_URL: &str = "https://auth.openai.com/oauth/authorize";
const TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
const REDIRECT_URI: &str = "http://localhost:1455/auth/callback";
const CALLBACK_PORT: u16 = 1455;
const CALLBACK_PATH: &str = "/auth/callback";
const SCOPES: &str = "openid profile email offline_access";

// ── Token response ──────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: String,
    #[serde(default)]
    expires_in: i64,
    #[serde(default)]
    id_token: Option<String>,
}

// ── Public API ──────────────────────────────────────────────────────

/// Run the full OpenAI Codex OAuth authorization-code + PKCE flow.
pub async fn login_openai_codex(callbacks: OAuthCallbacks) -> Result<OAuthCredentials> {
    let (verifier, challenge) = generate_pkce();

    let state = uuid::Uuid::new_v4().to_string();

    // Build auth URL — must match pi exactly (no audience param).
    let auth_url = format!(
        "{AUTHORIZE_URL}?\
         response_type=code\
         &client_id={CLIENT_ID}\
         &redirect_uri={redirect}\
         &scope={scopes}\
         &code_challenge={challenge}\
         &code_challenge_method=S256\
         &state={state}\
         &id_token_add_organizations=true\
         &codex_cli_simplified_flow=true\
         &originator=bb",
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

    exchange_code(&params.code, &verifier).await
}

/// Refresh an existing OpenAI Codex OAuth token.
pub async fn refresh_openai_codex_token(refresh_token: &str) -> Result<OAuthCredentials> {
    let client = reqwest::Client::new();
    let resp = client
        .post(TOKEN_URL)
        .form(&[
            ("grant_type", "refresh_token"),
            ("client_id", CLIENT_ID),
            ("refresh_token", refresh_token),
        ])
        .send()
        .await
        .context("Failed to send refresh request to OpenAI")?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("OpenAI token refresh failed ({status}): {body}");
    }

    let token: TokenResponse = resp.json().await.context("Failed to parse token response")?;
    let now_ms = chrono::Utc::now().timestamp_millis();

    let account_id = extract_account_id(&token.access_token);
    let extra = match account_id {
        Some(id) => serde_json::json!({ "accountId": id }),
        None => serde_json::Value::Null,
    };

    Ok(OAuthCredentials {
        access: token.access_token,
        refresh: token.refresh_token,
        expires: now_ms + token.expires_in * 1000,
        extra,
    })
}

// ── Internals ───────────────────────────────────────────────────────

async fn exchange_code(code: &str, verifier: &str) -> Result<OAuthCredentials> {
    let client = reqwest::Client::new();
    let resp = client
        .post(TOKEN_URL)
        .form(&[
            ("grant_type", "authorization_code"),
            ("client_id", CLIENT_ID),
            ("code", code),
            ("redirect_uri", REDIRECT_URI),
            ("code_verifier", verifier),
        ])
        .send()
        .await
        .context("Failed to send token exchange request to OpenAI")?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("OpenAI token exchange failed ({status}): {body}");
    }

    let token: TokenResponse = resp.json().await.context("Failed to parse token response")?;
    let now_ms = chrono::Utc::now().timestamp_millis();

    let account_id = extract_account_id(&token.access_token);
    let extra = match account_id {
        Some(id) => serde_json::json!({ "accountId": id }),
        None => serde_json::Value::Null,
    };

    Ok(OAuthCredentials {
        access: token.access_token,
        refresh: token.refresh_token,
        expires: now_ms + token.expires_in * 1000,
        extra,
    })
}

/// Extract `chatgpt_account_id` (or any `"https://api.openai.com/auth"` claim)
/// from a JWT access token by decoding the payload (middle segment).
fn extract_account_id(jwt: &str) -> Option<String> {
    let parts: Vec<&str> = jwt.split('.').collect();
    if parts.len() < 2 {
        return None;
    }
    let payload = URL_SAFE_NO_PAD.decode(parts[1]).ok()?;
    let json: serde_json::Value = serde_json::from_slice(&payload).ok()?;

    // Try direct field first
    if let Some(id) = json.get("chatgpt_account_id").and_then(|v| v.as_str()) {
        return Some(id.to_string());
    }

    // Try nested under "https://api.openai.com/auth"
    if let Some(auth) = json.get("https://api.openai.com/auth") {
        if let Some(id) = auth.get("account_id").and_then(|v| v.as_str()) {
            return Some(id.to_string());
        }
        if let Some(id) = auth.get("chatgpt_account_id").and_then(|v| v.as_str()) {
            return Some(id.to_string());
        }
    }

    None
}

// ── Input parsing (matches pi's parseAuthorizationInput) ────────────

struct ParsedInput {
    code: Option<String>,
    state: Option<String>,
}

/// Parse user-pasted input that could be:
/// - A full redirect URL: `http://localhost:1455/auth/callback?code=...&state=...`
/// - A `code=...&state=...` query string
/// - A `code#state` pair
/// - A bare authorization code
fn parse_authorization_input(input: &str) -> ParsedInput {
    let value = input.trim();
    if value.is_empty() {
        return ParsedInput { code: None, state: None };
    }

    // Try as URL
    if let Ok(url) = url::Url::parse(value) {
        let code = url.query_pairs().find(|(k, _)| k == "code").map(|(_, v)| v.to_string());
        let state = url.query_pairs().find(|(k, _)| k == "state").map(|(_, v)| v.to_string());
        if code.is_some() {
            return ParsedInput { code, state };
        }
    }

    // Try code#state
    if value.contains('#') {
        let parts: Vec<&str> = value.splitn(2, '#').collect();
        return ParsedInput {
            code: parts.first().map(|s| s.to_string()),
            state: parts.get(1).map(|s| s.to_string()),
        };
    }

    // Try query string with code=
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

    // Treat as bare code
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_account_id_from_jwt() {
        // Build a fake JWT with the account ID in the payload
        let header = URL_SAFE_NO_PAD.encode(b"{}");
        let payload_json = serde_json::json!({
            "https://api.openai.com/auth": {
                "account_id": "acct_test123"
            }
        });
        let payload = URL_SAFE_NO_PAD.encode(payload_json.to_string().as_bytes());
        let sig = URL_SAFE_NO_PAD.encode(b"sig");
        let jwt = format!("{header}.{payload}.{sig}");

        assert_eq!(extract_account_id(&jwt), Some("acct_test123".into()));
    }

    #[test]
    fn extract_account_id_missing() {
        assert_eq!(extract_account_id("not.a.jwt"), None);
    }
}
