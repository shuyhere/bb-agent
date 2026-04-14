pub mod anthropic;
pub mod callback_server;
pub mod github_copilot;
pub mod openai_codex;
pub mod pkce;

pub use anthropic::login_anthropic;
pub(crate) use github_copilot::login_github_copilot;
pub use openai_codex::login_openai_codex;

/// Credentials returned from a successful OAuth flow.
#[derive(Debug, Clone)]
pub struct OAuthCredentials {
    /// The access / bearer token.
    pub access: String,
    /// The refresh token (used to obtain new access tokens).
    pub refresh: String,
    /// Expiry as unix-epoch **milliseconds**.
    pub expires: i64,
    /// Provider-specific extras (e.g. `accountId` for OpenAI).
    pub extra: serde_json::Value,
}

#[derive(Debug, Clone)]
pub struct OAuthDeviceCode {
    pub user_code: String,
    pub verification_uri: String,
}

/// Callbacks the caller supplies so the OAuth helpers can interact with the
/// user without depending on any particular UI layer.
pub struct OAuthCallbacks {
    /// Called with the authorization URL the user must visit.
    pub on_auth: Box<dyn FnOnce(String) + Send>,
    /// Optional device-code presentation hook.
    pub on_device_code: Option<Box<dyn Fn(OAuthDeviceCode) + Send>>,
    /// If set, the OAuth flow will race the callback-server against this
    /// oneshot — the first value wins.  This lets the TUI offer a "paste
    /// code manually" fallback.
    pub on_manual_input: Option<tokio::sync::oneshot::Receiver<String>>,
    /// Optional progress updates (e.g. "Waiting for browser…").
    pub on_progress: Option<Box<dyn Fn(String) + Send>>,
}
