# Changelog

All notable changes to BB-Agent will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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

[0.0.10]: https://github.com/shuyhere/bb-agent/releases/tag/v0.0.10
[0.0.9]: https://github.com/shuyhere/bb-agent/releases/tag/v0.0.9
[0.0.8]: https://github.com/shuyhere/bb-agent/releases/tag/v0.0.8
