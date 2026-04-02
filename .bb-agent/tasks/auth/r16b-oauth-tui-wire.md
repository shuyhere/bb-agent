# Task: wire OAuth login into TUI and update auth selector

Worktree: `/tmp/bb-restructure/r16b-oauth-wire`
Branch: `r16b-oauth-tui-wire`

## Goal
Wire the OAuth flows from `crates/cli/src/oauth/` into the interactive TUI so `/login` works end-to-end.

## Prerequisites
This task depends on r16a-oauth-core being merged first. The OAuth module should already exist at `crates/cli/src/oauth/`.

## Changes needed

### 1. Update auth selector overlay (`crates/cli/src/interactive/auth_selector_overlay.rs`)
Show providers with their auth method:
```
  > Anthropic (OAuth)     [via bb]  (Enter to re-auth)
    OpenAI Codex (OAuth)  [not authenticated]
    Google (API key)      [not authenticated]
    Groq (API key)        [not authenticated]
    xAI (API key)         [not authenticated]
    OpenRouter (API key)  [not authenticated]
```

Each provider entry should indicate whether it uses OAuth or API key.

Add to the `PROVIDERS` list:
```rust
const PROVIDERS: &[(&str, &str, AuthMethod)] = &[
    ("anthropic", "Anthropic", AuthMethod::OAuth),
    ("openai-codex", "OpenAI Codex", AuthMethod::OAuth),
    ("google", "Google", AuthMethod::ApiKey),
    ("groq", "Groq", AuthMethod::ApiKey),
    ("xai", "xAI", AuthMethod::ApiKey),
    ("openrouter", "OpenRouter", AuthMethod::ApiKey),
];
```

### 2. Update login handler (`crates/cli/src/interactive/controller/model_actions.rs`)
When user selects an OAuth provider:
1. Show status: "Opening browser for Anthropic login..."
2. Open browser with auth URL via `xdg-open` (Linux) / `open` (macOS)
3. Show status: "Waiting for browser auth... (paste code or URL if browser is remote)"
4. Set editor to accept manual code paste as fallback
5. Race between: callback server receiving code vs user pasting code
6. Exchange code for tokens
7. Save to auth.json
8. Show status: "Logged in to Anthropic"
9. Update footer

When user selects an API key provider:
- Keep current behavior (prompt for key paste)

### 3. Update `resolve_api_key` in `crates/cli/src/login.rs`
- When auth entry is OAuth and token is expired, call the refresh function
- Map provider names: "openai-codex" -> use openai provider for model resolution
- "openai" in model registry should check "openai-codex" auth as well

### 4. Update main.rs `bb login` CLI command
- `bb login` with no args -> show provider list (OAuth + API key)
- `bb login anthropic` -> run Anthropic OAuth flow in terminal
- `bb login google` -> prompt for API key (same as now)
- Show progress messages during OAuth flow

## Constraints
- Do NOT break API key auth flow
- Open browser with `std::process::Command::new("xdg-open")` on Linux
- Show helpful error if callback server port is in use
- Support manual code paste fallback for remote/headless use

## Verification
```
cargo build -q
# Manual: bb login -> select Anthropic -> browser opens -> complete auth -> tokens saved
# Manual: bb login -> select Google -> paste API key -> saved
```

## Finish
```
git add -A && git commit -m "wire OAuth login into TUI: browser flow, manual fallback, auth selector"
```
