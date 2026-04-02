# Task: implement OAuth core (PKCE, callback server, Anthropic + OpenAI flows)

Worktree: `/tmp/bb-restructure/r16a-oauth-core`  
Branch: `r16a-oauth-core`

## Goal
Create the OAuth login infrastructure in `crates/cli/src/oauth/`. This does NOT wire it into the TUI yet — just the core flows that can be called from code.

## Files to create

### `crates/cli/src/oauth/mod.rs`
```rust
pub mod pkce;
pub mod callback_server;
pub mod anthropic;
pub mod openai_codex;

pub use anthropic::login_anthropic;
pub use openai_codex::login_openai_codex;
```

### `crates/cli/src/oauth/pkce.rs`
PKCE (Proof Key for Code Exchange) generation:
- `generate_pkce() -> (String, String)` returns (verifier, challenge)
- verifier: 128 random alphanumeric chars
- challenge: SHA-256 hash of verifier, base64url-encoded (no padding)
- Use `sha2` crate for hashing, `rand` for random bytes

### `crates/cli/src/oauth/callback_server.rs`
Local HTTP callback server using tokio TCP:
- `start_callback_server(port: u16, expected_path: &str) -> CallbackServer`
- Listens on `127.0.0.1:{port}`
- Waits for GET request to the expected path
- Extracts `code` and `state` query params from the URL
- Returns them via a channel/oneshot
- Has a `cancel()` method
- Responds with simple HTML success/error page
- Use raw HTTP parsing over `tokio::net::TcpListener` (no hyper dependency needed)

### `crates/cli/src/oauth/anthropic.rs`
Anthropic OAuth flow:
- Constants: client_id=`9d1c250a-e61b-44d9-88ed-5944d1962f5e`, authorize=`https://claude.ai/oauth/authorize`, token=`https://platform.claude.com/v1/oauth/token`, redirect=`http://localhost:53692/callback`, scopes=`org:create_api_key user:profile user:inference user:sessions:claude_code user:mcp_servers user:file_upload`
- `login_anthropic(callbacks: OAuthCallbacks) -> Result<OAuthCredentials>`
- Flow: generate PKCE -> build auth URL -> call `callbacks.on_auth(url)` -> start callback server on port 53692 -> wait for code (or manual paste via `callbacks.on_manual_input()`) -> exchange code for tokens at token URL -> return credentials
- Token exchange: POST to token URL with `grant_type=authorization_code`, `client_id`, `code`, `state`, `redirect_uri`, `code_verifier`
- `refresh_anthropic_token(refresh_token: &str) -> Result<OAuthCredentials>`

### `crates/cli/src/oauth/openai_codex.rs`
OpenAI Codex OAuth flow:
- Constants: client_id=`app_EMoamEEZ73f0CkXaXp7hrann`, authorize=`https://auth.openai.com/oauth/authorize`, token=`https://auth.openai.com/oauth/token`, redirect=`http://localhost:1455/auth/callback`, scope=`openid profile email offline_access`
- `login_openai_codex(callbacks: OAuthCallbacks) -> Result<OAuthCredentials>`
- Same pattern as Anthropic but different URLs/ports
- After token exchange, extract `chatgpt_account_id` from JWT payload and store in credentials
- `refresh_openai_codex_token(refresh_token: &str) -> Result<OAuthCredentials>`

### Shared types (put in `mod.rs` or a `types.rs`)
```rust
pub struct OAuthCredentials {
    pub access: String,
    pub refresh: String,
    pub expires: i64,  // unix millis
    pub extra: serde_json::Value,  // for accountId etc
}

pub struct OAuthCallbacks {
    pub on_auth: Box<dyn FnOnce(String) + Send>,  // called with auth URL
    pub on_manual_input: Option<tokio::sync::oneshot::Sender<String>>,  // for manual code paste
    pub on_progress: Option<Box<dyn Fn(String) + Send>>,
}
```

## Also update `crates/cli/src/login.rs`
1. Change `AuthEntry` to support OAuth with pi-compatible fields:
```rust
#[serde(rename = "oauth")]
OAuth {
    access: String,
    refresh: String,
    expires: i64,
    #[serde(flatten)]
    extra: serde_json::Value,
}
```
2. Update `resolve_api_key()` to check OAuth token expiry and auto-refresh
3. Add `save_oauth_credentials(provider: &str, creds: &OAuthCredentials) -> Result<()>`

## Dependencies to add to `crates/cli/Cargo.toml`
- `sha2 = "0.10"`
- `rand = "0.8"`

## Constraints
- Do NOT wire into TUI yet (that's a separate task)
- Do NOT break existing API key auth
- Use same constants/endpoints as pi
- Keep OAuth format compatible with pi's auth.json

## Verification
```
cargo build -q
```

## Finish
```
git add -A && git commit -m "implement OAuth core: PKCE, callback server, Anthropic and OpenAI Codex flows"
```
