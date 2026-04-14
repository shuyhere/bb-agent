# Changelog

All notable changes to BB-Agent will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Improved

- npm install now prefers compressed `.gz` GitHub release assets when available, falling back to the legacy uncompressed binaries for older releases
- release builds now strip debug info before publishing, substantially reducing native binary download size for npm installs and updates

## [0.0.15] - 2026-04-12

### Fixed

- tui bash output now streams live stdout/stderr and keeps running, finished, historical, and expanded/collapsed tool blocks visually consistent with width-aware tail previews and elapsed/took footer hints
- bash tool titles now skip shell prelude lines like `set -e`, show configured timeout values in the UI, and validate invalid timeout arguments instead of accepting zero or non-finite values
- fixed the session getting stuck after interrupted or failed tool calls by flushing synthetic tool results before later prompts, preventing follow-up turns from failing with missing tool output
- Codex tool-call request handling is more robust: tool calls are serialized sequentially, orphan tool results are sanitized out of requests, and streamed/done function-call events are deduplicated more safely
- plain URLs and hyphenated text no longer trigger accidental markdown horizontal-rule rendering while explicit markdown rules and setext headings still render correctly
- tui resume menu handling now awaits the async resume path correctly instead of dropping back through the synchronous menu flow

### Added

- regression coverage for interrupted tool-call recovery, Codex orphan-tool sanitization, builtin tool-name normalization, tui bash rendering consistency, and bash timeout validation/visibility

## [0.0.14] - 2026-04-12

### Added

- tui now supports extension-driven workflows and structured slash-command outcomes, including menus, hidden dispatches, and richer command result handling
- `/settings` in tui now exposes compaction controls for `Auto-compact`, `Reserve tokens`, and `Keep recent tokens`
- skills can now be listed, disabled, and re-enabled from the CLI without deleting their installed files
- startup model selection now prefers configured provider/model defaults more consistently, with better OpenAI startup fallback behavior
- added a parity test script against installed pi compaction logic to keep BB token accounting aligned with upstream behavior

### Fixed

- session resume now restores the prior model and thinking level instead of starting with mismatched runtime defaults
- tui/TUI terminal rendering now sanitizes terminal control text more reliably and avoids ANSI leakage into the UI
- auto-compaction token estimation now matches pi more closely by using the last successful assistant usage plus trailing estimates, using ceil-based token heuristics, computing `tokens_before` from rebuilt context instead of raw payload size, and ignoring assistant usage from before the latest compaction boundary
- tui compaction behavior and status reporting are more consistent after auto-compaction and manual compaction events, and local tui actions now show an animated elapsed-time status while they run

### Changed

- tui extension workflows and session compaction support are now merged into the main interaction path on `master`

## [0.0.13] - 2026-04-09

### Added

- tui screenshot and image clipboard paste now works on the normal paste path, with macOS clipboard fallbacks and Codex image preservation so pasted images reach image-capable models correctly
- model registry metadata now tracks image input capability, making `/models` truthful about image support and allowing runtime warnings when users attach images to text-only models

### Fixed

- tui clipboard image attach no longer leaks helper `true` / `false` output or stray follow-up paste text into the input block
- attached image chips can now be removed with `Backspace`, image-only prompts can be submitted, and optimistic user messages keep attachment chip previews in the transcript
- rebuilt tui session transcripts now preserve user image attachment markers instead of silently dropping image blocks
- managed `bb-clipboard-*.png` temp files are now cleaned up after removal or ingestion instead of lingering in `/tmp`
- the tui input block now hides raw `@file` tokens when the corresponding attachment chip is already shown, preventing duplicated `@file` text in the editor
- tui tool-header regression tests now match the intended live bash-header rendering and running-dot animation behavior

## [0.0.12] - 2026-04-06

### Fixed

- direct `@image` references in print mode and tui now attach real image inputs instead of falling back to UTF-8 read warnings
- `@path with spaces` parsing now correctly keeps the full file path before trailing prompt text, including whole-message forms
- image tool results are now preserved through provider conversion so models can actually see images returned by tools instead of responding as if no image was provided
- tui `@` folder navigation now keeps the completion menu open when you select a directory and immediately shows the next level, including directories with spaces
- the tui input block now shows attached files as `[name, sizeKB]`, keeps those chips visible, and places the cursor below them so typing starts after the attachments

### Changed

- binary office/document inputs (`pdf`, `docx`, `pptx`, `xlsx`) now degrade to format-aware metadata notes instead of misleading invalid-UTF-8 errors

## [0.0.11] - 2026-04-07

### Added

- startup update notices in the tui transcript are now highlighted so available updates stand out clearly during startup
- read-tool line ranges in tui tool activity now highlight the requested span, so values like `2148-2267/5006` stand out while the model is using tools
- tui footer and `/session` info now show the active execution posture so safety vs yolo is visible during a run

### Improved

- npm install now caches verified native binaries by version/platform and reuses them on reinstall instead of re-downloading every time
- npm install now shows more frequent download progress with transfer rate information to make slow installs easier to understand
- npm install now avoids unnecessary re-verification on cache hits, making repeat installs faster
- safety mode now restricts built-in `write` and `edit` to the active workspace, while `yolo` keeps unrestricted file mutation behavior

### Migration

- `execution_mode` now defaults to `safety`; set `"execution_mode": "yolo"` if your workflow intentionally edits files outside the current workspace

## [0.0.10] - 2026-04-07

### Fixed

- npm install now uses a longer timeout, retries release-binary downloads, and reports real download errors instead of incorrectly saying no matching prebuilt binary exists
- npm install now shows progress logs during native binary download and verification so first-time installs on macOS/Linux are less confusing
- tui `/login` provider-family status now correctly shows OpenAI OAuth state after ChatGPT login instead of incorrectly showing the API key path as not authenticated

### Changed

- README install docs now lead with `npm install -g @shuyhere/bb-agent`, move terminal/font guidance into Troubleshooting, and clearly separate npm install from building from source for development

## [0.0.9] - 2026-04-07

### Added

- `@folder/` expansion now sends a directory tree summary to the model instead of treating folders like text files
- large `@file` expansions now send a structural outline first for long files instead of dumping the entire file immediately
- non-UTF-8 and binary `@file` references now send metadata instead of a misleading UTF-8 read error

### Fixed

- tui paste in iTerm2/SSH no longer corrupts the input area after paste
- pasted file and image paths are normalized more reliably, including quoted paths and `file://` URLs
- tui prompt submission now expands `@file` references consistently
- running tool timers continue updating after `TurnEnd` while tools are still executing
- sub-second tool durations now display as `ms` instead of `0.0s`
- startup Skills/Prompts/Extensions note now only appears at startup or explicit `/reload`
- remote SSH clipboard copy no longer leaks Wayland/XDG clipboard helper warnings into the TUI

### Changed

- tui `Ctrl+V` now falls back to clipboard text when no clipboard image is available
- `@` autocomplete now inserts quoted file references when paths contain spaces

## [0.0.8] - 2026-04-06

### Fixed

- auth-aware startup now prefers configured defaults or the last authenticated provider/model instead of falling back unexpectedly
- Gemini default model now prefers `gemini-3.1-pro`
- GitHub Copilot default model selection now prefers Claude Opus 4.6 when available
- login and no-auth UX now remind users that `/model` can switch to other configured models
- startup now shows a short update notice with npm-aware update commands when installed from npm

### Added

- startup update notice for published builds, including npm-specific upgrade guidance

### Changed

- latest published package includes the post-0.0.7 startup, auth, model-default, and update-notice improvements

[0.0.14]: https://github.com/shuyhere/bb-agent/releases/tag/v0.0.14
[0.0.13]: https://github.com/shuyhere/bb-agent/releases/tag/v0.0.13
[0.0.12]: https://github.com/shuyhere/bb-agent/releases/tag/v0.0.12
[0.0.11]: https://github.com/shuyhere/bb-agent/releases/tag/v0.0.11
[0.0.10]: https://github.com/shuyhere/bb-agent/releases/tag/v0.0.10
[0.0.9]: https://github.com/shuyhere/bb-agent/releases/tag/v0.0.9
[0.0.8]: https://github.com/shuyhere/bb-agent/releases/tag/v0.0.8
