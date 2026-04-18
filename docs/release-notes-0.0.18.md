# Release notes draft — v0.0.18

_Date: 2026-04-18_

## Highlights

### KV-cache reporting now matches auth reality more closely
This release tightens the cache-monitoring story that landed in `v0.0.17`:
- API-key sessions now report cache-hit provenance as **official**
- OAuth sessions now report cache-hit provenance as **estimate**
- OpenAI and Anthropic baseline requests restore provider-side cache-affinity shaping to make cache behavior more meaningful during repeated prompts
- estimated cache-hit normalization is tightened so changed prompts no longer falsely peg at `100.0%`

The result is a cache monitor that is much more useful for real-world validation across auth modes.

### Cache monitor placement and wording are clearer
The TUI cache monitor now renders on the footer/path line and labels its provenance directly:
- `cache hit (official)`
- `cache hit (estimate)`
- `cache hit (mixed)`
- `cache hit (unknown)`

This makes it much easier to tell whether you are looking at provider-reported cache data or BB's estimated reuse path.

### Model/auth switching now resets stale latest-hit state
When you switch models — or switch auth sources for the same model — the cache monitor now starts cold instead of showing the previous cache domain's latest-hit/source values.

This avoids misleading transitions like:
- previous model's latest hit rate carrying over after `/model`
- previous auth source's cache provenance remaining visible after an in-session auth switch

### Small TUI usability polish: `/exit`
The shared TUI slash-command registry now includes `/exit` as an alias for `/quit`, so help text, menus, and slash matching all expose the more familiar exit command consistently.

## Notable user-facing changes

### Added
- `/exit` as a shared TUI slash-command alias for `/quit`

### Changed
- cache metrics now follow auth mode explicitly (`official` for API key, `estimate` for OAuth)
- the cache monitor now renders on the footer/path line with clearer provenance wording
- OpenAI and Anthropic cache-affinity shaping is restored for better cache baseline behavior

### Fixed
- estimated latest-hit reporting no longer overstates changed prompts as full cache hits
- `/model` switches now reset stale latest-hit cache monitor state
- in-session auth-source switches now also reset stale latest-hit/source cache monitor state

## Upgrade notes
- No manual migration is required.
- Existing cache/session data continues to load, but the TUI monitor may now report different provenance labels and more conservative latest-hit values for OAuth flows.
- If you are manually validating cache behavior, compare API-key sessions against OAuth sessions with the expectation that API-key paths are the ground-truth baseline.

## Suggested GitHub release summary
BB-Agent v0.0.18 tightens the KV-cache validation loop: cache provenance now follows auth mode explicitly, estimated hit rates are better normalized, stale latest-hit state is reset on model/auth switches, the TUI cache monitor is clearer about what it is showing, and `/exit` is now available as a shared TUI slash-command alias.

## Final clean build/test matrix

Performed from a clean `origin/master`-based worktree for `release/0.0.18` prep.

| Area | Command | Result |
| --- | --- | --- |
| Build | `cargo +stable build -q -p bb-cli --bin bb` | Passed |
| Slash-command alias tests | `cargo +stable test -q -p bb-tui slash_commands -- --nocapture` | Passed |
| Provider cache source (OAuth) | `cargo +stable test -q -p bb-provider oauth_usage_is_marked_as_estimated -- --nocapture` | Passed |
| Responses body conversion | `cargo +stable test -q -p bb-provider responses_body_converts_chat_style_tools_and_system_messages -- --nocapture` | Passed |
| Anthropic usage provenance | `cargo +stable test -q -p bb-provider usage_events_preserve_requested_cache_metric_source -- --nocapture` | Passed |
| Estimated normalization | `cargo +stable test -q -p bb-monitor normalized_estimate_does_not_peg_changed_prompts_to_hundred_percent -- --nocapture` | Passed |
| Tracker reset | `cargo +stable test -q -p bb-monitor reset_history_clears_previous_prompt_and_bumps_epoch -- --nocapture` | Passed |
| TUI stale latest-hit reset | `cargo +stable test -q -p bb-cli stale_request_metrics_do_not_match_new_cache_domain -- --nocapture` | Passed |
| TUI model-switch stale latest-hit reset | `cargo +stable test -q -p bb-cli model_mismatch_uses_current_auth_source_and_zeroes_latest -- --nocapture` | Passed |
| Release binary smoke test | `./target/debug/bb --help` | Passed |

### Notes
- Validation was run with `CARGO_BUILD_JOBS=1`, `CARGO_INCREMENTAL=0`, and `RUSTFLAGS='-Cdebuginfo=0'` to reduce host-side Rust instability during release prep.
- A tiny release-prep fix was included to unblock clean `bb-tui` test builds by narrowing a stale test-only `wrap_visual_line` re-export in `crates/tui/src/tui/projection.rs`.
- Rebuild from the clean release worktree before tagging/publishing so the installed binary matches the exact merged release commit.
