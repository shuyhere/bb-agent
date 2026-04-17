# Release notes draft — v0.0.17

_Date: 2026-04-17_

## Highlights

### Cache monitoring is now a first-class backend + TUI feature
This release lands the full cache-monitoring stack that was still in progress after `v0.0.16`:
- reusable backend monitor logic now lives in `crates/bb-monitor`
- request metrics, usage rollups, and cache provenance tracking are centralized there
- the TUI now shows a dedicated cache monitor line directly under the input block
- that monitor now includes:
  - cache source
  - average cache hit rate
  - latest request hit rate

This makes cache behavior much easier to reason about without digging through logs or guessing whether zero values are coming from provider data, persistence, or UI rendering.

### Auth/provider UX is much more explicit
Auth handling is now visible and controllable in the core user flows:
- footer auth badges now show explicit method/source state such as OAuth vs API key
- `/session` shows explicit auth source, method, account, and authority
- `/model` can choose auth source/profile, not just provider/model
- `/login` now exposes concrete auth-option choosers instead of only coarse method-level picks

This release also introduces a profile-aware auth store so saved auth can be tracked and switched more cleanly over time.

### Multiple saved auth profiles per provider
Providers that support it can now keep more than one saved auth profile side-by-side:
- multiple saved OAuth profiles per provider
- multiple saved API-key profiles per provider
- saved API keys are labeled safely using non-secret suffixes such as `ending in 2222`
- auth choosers also include a short saved-profile discriminator so similar-looking entries are still distinguishable

For example, you can now keep multiple saved OpenAI or OpenRouter API keys instead of replacing the previous saved key every time.

### GPT-5 API-key requests now use the correct OpenAI endpoint
OpenAI GPT-5 API-key sessions now route tool/reasoning requests through the Responses API instead of `/v1/chat/completions`, fixing provider-side `reasoning_effort` request failures.

## Notable user-facing changes

### Added
- `crates/bb-monitor` as a reusable backend monitor crate
- under-input TUI cache monitor
- profile-aware auth metadata and timestamps
- multiple saved API-key profiles per provider

### Changed
- `/model` now supports auth source/profile choice as part of model selection
- `/login` now acts as a real auth option/profile chooser instead of only a method picker
- auth state is presented more consistently across footer, `/session`, `/model`, and `/login`

### Fixed
- GPT-5 API-key requests now use OpenAI Responses API when required
- `/session` now reflects explicit in-session auth selection correctly
- multi-key auth menus now avoid ambiguous saved-key entries by adding safe labels and saved-profile discriminators

## Upgrade notes
- Existing `auth.json` stores continue to migrate automatically; no manual auth-store migration is required.
- If you already have saved provider auth, BB will continue to load it, but richer profile metadata may now appear in `/login`, `/model`, and `/session`.
- Distinct saved API keys for the same provider are now preserved as separate profiles instead of overwriting one another.

## Suggested GitHub release summary
BB-Agent v0.0.17 focuses on visibility and control: cache behavior is now first-class in both the backend and TUI, auth source/profile selection is explicit across `/model`, `/login`, and `/session`, multiple saved auth profiles can coexist cleanly, and GPT-5 API-key requests now use OpenAI’s correct Responses API path.

## Final clean build/test matrix

Performed from a clean `origin/master`-based worktree for `release/0.0.17` prep.

| Area | Command | Result |
| --- | --- | --- |
| Build | `cargo +stable build -q -p bb-cli --bin bb` | Passed |
| CLI auth tests | `cargo +stable test -q -p bb-cli login -- --nocapture` | Passed |
| Session info auth tests | `cargo +stable test -q -p bb-cli session_summary -- --nocapture` | Passed |
| OpenAI provider tests | `cargo +stable test -q -p bb-provider openai -- --nocapture` | Passed |
| Multi saved API-key chooser | `cargo +stable test -q -p bb-cli model_auth_menu_distinguishes_multiple_saved_api_keys -- --nocapture` | Passed |
| Multi saved API-key summaries | `cargo +stable test -q -p bb-cli provider_auth_option_summaries_distinguish_multiple_saved_api_keys -- --nocapture` | Passed |
| Release binary smoke test | `./target/debug/bb --help` | Passed |

### Notes
- Commands are run with `CARGO_BUILD_JOBS=1`, `CARGO_INCREMENTAL=0`, and `RUSTFLAGS='-Cdebuginfo=0'` to reduce host-side Rust instability during validation.
- After the version bump, rerun the final build and smoke checks before tagging/publishing.
