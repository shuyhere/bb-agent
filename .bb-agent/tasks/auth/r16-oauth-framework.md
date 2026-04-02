# Task: implement OAuth login framework for BB-Agent

Worktree: `/tmp/bb-restructure/r16-oauth`
Branch: `r16-oauth-framework`

## Goal
BB-Agent currently only supports API key paste for login. Pi supports full OAuth flows for:
- **Anthropic** (Claude Pro/Max) — PKCE flow via claude.ai
- **OpenAI Codex** (ChatGPT Plus/Pro) — PKCE flow via auth.openai.com

Implement the OAuth login framework so `/login` in BB offers both OAuth and API key auth.

## Architecture

### Auth storage format (align with pi)
`~/.bb-agent/auth.json` should store:
```json
{
  "anthropic": {
    "type": "oauth",
    "access": "sk-ant-oat01-...",
    "refresh": "...",
    "expires": 1775000000000
  },
  "openai-codex": {
    "type": "oauth",
    "access": "eyJhbG...",
    "refresh": "...",
    "expires": 1775000000000,
    "accountId": "..."
  },
  "google": {
    "type": "api_key",
    "key": "AIza..."
  }
}
```

### OAuth flow (for each provider)
1. Generate PKCE challenge (code_verifier + code_challenge)
2. Build authorization URL with client_id, redirect_uri, scope, PKCE challenge
3. Open browser (`xdg-open` on Linux)
4. Start local HTTP server listening for callback
5. Wait for redirect with auth code (or user pastes code/URL manually)
6. Exchange code for access_token + refresh_token
7. Save to auth.json with `type: "oauth"` and expiry

### Provider constants (from pi source)

**Anthropic:**
- client_id: `9d1c250a-e61b-44d9-88ed-5944d1962f5e`
- authorize_url: `https://claude.ai/oauth/authorize`
- token_url: `https://platform.claude.com/v1/oauth/token`
- redirect_uri: `http://localhost:53692/callback`
- scopes: `org:create_api_key user:profile user:inference user:sessions:claude_code user:mcp_servers user:file_upload`

**OpenAI Codex:**
- client_id: `app_EMoamEEZ73f0CkXaXp7hrann`
- authorize_url: `https://auth.openai.com/oauth/authorize`
- token_url: `https://auth.openai.com/oauth/token`
- redirect_uri: `http://localhost:1455/auth/callback`
- scope: `openid profile email offline_access`

### Files to create/modify

1. **`crates/cli/src/oauth/mod.rs`** — OAuth framework
2. **`crates/cli/src/oauth/pkce.rs`** — PKCE challenge generation (SHA-256 + base64url)
3. **`crates/cli/src/oauth/anthropic.rs`** — Anthropic OAuth flow
4. **`crates/cli/src/oauth/openai.rs`** — OpenAI Codex OAuth flow
5. **`crates/cli/src/oauth/callback_server.rs`** — Local HTTP callback server
6. **`crates/cli/src/login.rs`** — Update auth storage to handle both `api_key` and `oauth` types, add token refresh
7. **`crates/cli/src/interactive/auth_selector_overlay.rs`** — Update to show OAuth vs API key options
8. **`crates/cli/src/interactive/controller/model_actions.rs`** — Update login handler to run OAuth flow

### PKCE implementation
```rust
use sha2::{Sha256, Digest};
use rand::Rng;

fn generate_pkce() -> (String, String) {
    let verifier: String = rand::thread_rng()
        .sample_iter(&rand::distributions::Alphanumeric)
        .take(128)
        .map(char::from)
        .collect();
    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    let hash = hasher.finalize();
    let challenge = base64_url_encode(&hash);
    (verifier, challenge)
}
```

### Callback server
Use `tokio::net::TcpListener` + `hyper` or raw HTTP parsing to listen on localhost for the redirect.

### Token refresh
When `resolve_api_key` finds an OAuth entry with `expires < now`, call the token refresh endpoint before returning the access token.

### Dependencies to add
- `sha2` for PKCE
- `rand` for verifier generation  
- `base64` (already present in provider crate)

## Auth selector UI update
The `/login` selector should show:
```
  > Anthropic (OAuth)  [via bb]
    OpenAI Codex (OAuth)
    Google (API key)
    Groq (API key)
    xAI (API key)
    OpenRouter (API key)
```

For OAuth providers: Enter opens browser flow
For API key providers: Enter prompts for key paste (current behavior)

## Constraints
- Keep the existing API key flow working.
- Do NOT break existing `resolve_api_key()` behavior.
- Use the SAME client IDs and endpoints as pi (they are public OAuth clients).
- Store in the SAME format as pi so tokens are interchangeable.

## Verification
```
cargo build -q
# Manual test: bb login -> select Anthropic -> browser opens -> auth completes
```

## Finish
```
git add -A && git commit -m "implement OAuth login framework for Anthropic and OpenAI Codex"
```
