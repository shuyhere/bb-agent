# Changelog

All notable changes to BB-Agent will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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

[0.0.8]: https://github.com/shuyhere/bb-agent/releases/tag/v0.0.8
