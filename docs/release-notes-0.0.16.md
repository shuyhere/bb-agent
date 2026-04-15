# Release notes draft — v0.0.16

_Date: 2026-04-15_

## Highlights

### TUI polish and usability
- The product/UI terminology now consistently uses **TUI** instead of legacy `fullscreen` wording.
- Markdown code blocks and bash tool previews now render as raw fenced blocks, making copy/paste much easier.
- TUI status/footer handling is more trustworthy:
  - queued previews no longer hide active compaction/local-action status
  - fast local actions no longer flash noisy `0ms` / `0.0s`
  - footer context usage no longer gets stuck at misleading zero-like values after resume/rebuild/fork flows

### Better browser setup for `browser_fetch`
- `browser_fetch` now reports missing-browser problems with clearer diagnostics.
- New command: `bb setup browser`
  - explains what is missing
  - shows platform-specific installation guidance
  - can optionally persist `BB_BROWSER` shell configuration

### Tool/runtime reliability architecture
This release folds in the full architecture cleanup tracked by issue #74:
- active tool visibility and advertised schemas now come from one CLI `ToolRegistry`
- provider transcript validation/repair is centralized before serialization
- tool execution follows an explicit lifecycle
- new lifecycle hooks exist for:
  - `tool_execution_start`
  - `tool_execution_update`
  - `tool_execution_end`
- mutation-aware scheduling now allows safe overlapping read-only work while serializing same-file mutation windows

This closes several long-running reliability issues around:
- `Unknown tool` drift
- stuck sessions after failed/interrupted tool calls
- provider-side `No tool output found ...` follow-up failures
- TUI turns appearing finished before cleanup actually completed

### Release hardening
- secret-bearing auth/token structs now redact sensitive fields from `Debug` output
- clean `bb-cli` release builds no longer emit dead-code warnings from the new tool registry surface
- npm/native release artifacts now prefer stripped/compressed binaries for smaller downloads

## Notable user-facing changes

### Added
- `bb setup browser`
- better missing-browser guidance for `browser_fetch`

### Changed
- `fullscreen` terminology is now fully replaced by **TUI**
- TUI code and bash rendering is more copy/paste-friendly
- internal tool/runtime execution is more deterministic and resilient

### Fixed
- footer context token usage reporting in TUI
- compaction/local-action status visibility when prompts are queued
- misleading `0ms` / `0.0s` local-action flashes
- stuck follow-up turns after interrupted/failed tool calls
- join-timeout handling that used to visually end the turn too early
- raw debug exposure risk for token-bearing auth structs

## Upgrade notes
- No migration action is required for normal users.
- If you use `browser_fetch`, you may now prefer the guided setup flow:

```bash
bb setup browser
```

- Existing auth stores continue to work as before; this release only hardens debug redaction, not the stored auth schema.

## Suggested GitHub release summary
BB-Agent v0.0.16 focuses on reliability and release polish: the TUI is easier to read and copy from, browser setup is guided, tool execution and transcript repair are now architecture-level invariants instead of scattered fixes, and release builds are hardened against both dead-code warnings and accidental token exposure in debug output.

## Final clean build/test matrix

Performed from a clean `origin/master`-based worktree before release prep finalization.

| Area | Command | Result |
| --- | --- | --- |
| Build | `cargo +stable build -q -p bb-cli --bin bb` | Passed |
| Release binary smoke test | `./target/debug/bb --help` | Passed |
| Full CLI test suite | `cargo +stable test -q -p bb-cli --bins --tests` | Passed (`126 passed`) |
| Core library tests | `cargo +stable test -q -p bb-core --lib` | Passed (`59 passed`) |
| Tools scheduler tests | `cargo +stable test -q -p bb-tools scheduler -- --nocapture` | Passed (`2 passed`) |
| Plugin-host lifecycle serialization tests | `cargo +stable test -q -p bb-plugin-host test_serialize_event -- --nocapture` | Passed (`4 passed`) |
| CLI tool registry tests | `cargo +stable test -q -p bb-cli tool_registry -- --nocapture` | Passed (`4 passed`) |
| OAuth debug redaction | `cargo +stable test -q -p bb-cli oauth_credentials_debug_redacts_tokens -- --nocapture` | Passed (`1 passed`) |
| GitHub/Copilot debug redaction | `cargo +stable test -q -p bb-cli debug_output_redacts_github_and_copilot_tokens -- --nocapture` | Passed (`1 passed`) |
| Auth entry redaction | `cargo +stable test -q -p bb-cli auth_entry_debug_redacts_secret_fields -- --nocapture` | Passed (`1 passed`) |
| Auth store redaction | `cargo +stable test -q -p bb-cli auth_store_debug_lists_provider_names_without_values -- --nocapture` | Passed (`1 passed`) |

### Notes
- Commands were run with `CARGO_BUILD_JOBS=1`, `CARGO_INCREMENTAL=0`, and `RUSTFLAGS='-Cdebuginfo=0'` to reduce host-side Rust instability during validation.
- One passing CLI test intentionally exercises an npm 404 path and therefore prints an expected registry-not-found message while still passing.
