# Changelog

All notable changes to BB-Agent will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.0.13] - 2026-04-09

### Added

- fullscreen screenshot and image clipboard paste now works on the normal paste path, with macOS clipboard fallbacks and Codex image preservation so pasted images reach image-capable models correctly
- model registry metadata now tracks image input capability, making `/models` truthful about image support and allowing runtime warnings when users attach images to text-only models

### Fixed

- fullscreen clipboard image attach no longer leaks helper `true` / `false` output or stray follow-up paste text into the input block
- attached image chips can now be removed with `Backspace`, image-only prompts can be submitted, and optimistic user messages keep attachment chip previews in the transcript
- rebuilt fullscreen session transcripts now preserve user image attachment markers instead of silently dropping image blocks
- managed `bb-clipboard-*.png` temp files are now cleaned up after removal or ingestion instead of lingering in `/tmp`
- the fullscreen input block now hides raw `@file` tokens when the corresponding attachment chip is already shown, preventing duplicated `@file` text in the editor
- fullscreen tool-header regression tests now match the intended live bash-header rendering and running-dot animation behavior

## [0.0.12] - 2026-04-06

### Fixed

- direct `@image` references in print mode and fullscreen now attach real image inputs instead of falling back to UTF-8 read warnings
- `@path with spaces` parsing now correctly keeps the full file path before trailing prompt text, including whole-message forms
- image tool results are now preserved through provider conversion so models can actually see images returned by tools instead of responding as if no image was provided
- fullscreen `@` folder navigation now keeps the completion menu open when you select a directory and immediately shows the next level, including directories with spaces
- the fullscreen input block now shows attached files as `[name, sizeKB]`, keeps those chips visible, and places the cursor below them so typing starts after the attachments

### Changed

- binary office/document inputs (`pdf`, `docx`, `pptx`, `xlsx`) now degrade to format-aware metadata notes instead of misleading invalid-UTF-8 errors

## [0.0.11] - 2026-04-07

### Added

- startup update notices in the fullscreen transcript are now highlighted so available updates stand out clearly during startup
- read-tool line ranges in fullscreen tool activity now highlight the requested span, so values like `2148-2267/5006` stand out while the model is using tools
- fullscreen footer and `/session` info now show the active execution posture so safety vs yolo is visible during a run

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
- fullscreen `/login` provider-family status now correctly shows OpenAI OAuth state after ChatGPT login instead of incorrectly showing the API key path as not authenticated

### Changed

- README install docs now lead with `npm install -g @shuyhere/bb-agent`, move terminal/font guidance into Troubleshooting, and clearly separate npm install from building from source for development

## [0.0.9] - 2026-04-07

### Added

- `@folder/` expansion now sends a directory tree summary to the model instead of treating folders like text files
- large `@file` expansions now send a structural outline first for long files instead of dumping the entire file immediately
- non-UTF-8 and binary `@file` references now send metadata instead of a misleading UTF-8 read error

### Fixed

- fullscreen paste in iTerm2/SSH no longer corrupts the input area after paste
- pasted file and image paths are normalized more reliably, including quoted paths and `file://` URLs
- fullscreen prompt submission now expands `@file` references consistently
- running tool timers continue updating after `TurnEnd` while tools are still executing
- sub-second tool durations now display as `ms` instead of `0.0s`
- startup Skills/Prompts/Extensions note now only appears at startup or explicit `/reload`
- remote SSH clipboard copy no longer leaks Wayland/XDG clipboard helper warnings into the TUI

### Changed

- fullscreen `Ctrl+V` now falls back to clipboard text when no clipboard image is available
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

[0.0.13]: https://github.com/shuyhere/bb-agent/releases/tag/v0.0.13
[0.0.12]: https://github.com/shuyhere/bb-agent/releases/tag/v0.0.12
[0.0.11]: https://github.com/shuyhere/bb-agent/releases/tag/v0.0.11
[0.0.10]: https://github.com/shuyhere/bb-agent/releases/tag/v0.0.10
[0.0.9]: https://github.com/shuyhere/bb-agent/releases/tag/v0.0.9
[0.0.8]: https://github.com/shuyhere/bb-agent/releases/tag/v0.0.8
