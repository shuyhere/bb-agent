# Release notes draft — v0.0.20

_Date: 2026-05-06_

## Highlights

### Builtin model registry refreshed
This release refreshes BB-Agent's builtin model registry across the major bundled providers:
- Anthropic
- OpenAI
- GitHub Copilot
- Google
- Groq
- OpenRouter

The updated registry includes newer model IDs, context/output limits, cost metadata, reasoning-capability flags, input-mode metadata, and default provider base URLs. Startup and login defaults now prefer the refreshed model IDs, including updated OpenAI/GitHub Copilot and Google defaults.

### Shape extension package added
`v0.0.20` includes the reviewed Shape extension package under `extensions/shape/`.

The package adds:
- a Shape skill with reference material
- a JavaScript extension entrypoint
- a Python knowledge-search helper
- extension tests and package metadata

### Provider docs updated
Provider documentation and README examples were updated to reflect the refreshed model list and newer default model examples.

## Notable user-facing changes

### Added
- added the reviewed Shape extension package with its bundled skill, references, extension entrypoint, helper tool, and tests

### Changed
- refreshed builtin model choices for Anthropic, OpenAI, GitHub Copilot, Google, Groq, and OpenRouter
- updated default model examples and provider docs to newer model IDs
- updated Google default selection to `gemini-3.1-pro-preview`
- updated OpenAI/GitHub Copilot default selection to `gpt-5.4`

### Improved
- added coverage for default model argument selection across core providers
- added representative registry coverage for refreshed builtin provider models

## Upgrade notes
- No manual migration is required.
- Existing provider settings continue to work.
- If your config pins an older model ID, BB-Agent will continue using that configured model unless you change it.
- New sessions that rely on provider defaults may select newer models than previous versions.

## Suggested GitHub release summary
BB-Agent v0.0.20 refreshes the builtin model registry across Anthropic, OpenAI, GitHub Copilot, Google, Groq, and OpenRouter, updates provider/default model documentation, and adds the reviewed Shape extension package.

## Final clean build/test matrix

Performed from an `origin/master`-based worktree for `release/0.0.20` prep.

| Area | Command | Result |
| --- | --- | --- |
| Formatting check | `cargo fmt --all -- --check` | Failed with existing Rust formatting drift across multiple files; not applied during release prep |
| Core tests | `cargo test -p bb-core` | Passed (`61 passed`) |
| Provider tests, serial | `cargo test -p bb-provider -- --test-threads=1` | Passed (`71 passed`) |
| CLI build | `cargo build -p bb-cli --bin bb` | Passed |
| Version smoke test | `./target/debug/bb --version` | Passed (`bb 0.0.20`) |
| npm package dry run | `npm pack --dry-run` | Passed (`@shuyhere/bb-agent@0.0.20`) |
| Provider tests, default parallel mode | `cargo test -p bb-provider` | Fails on `master` and release prep in `anthropic::events::tests::parses_server_tool_use_events` due to existing order/concurrency-sensitive test behavior |
| Full workspace tests | `cargo test` | Fails on `master` and release prep in `bb-session` test `Usage` initializers missing `cache_metrics_source` |

### Notes
- The model-list PR was merged as PR #143 before release prep.
- Parallel provider-test and full-workspace failures were reproduced on `master` before release prep and are not introduced by the model-list update.
