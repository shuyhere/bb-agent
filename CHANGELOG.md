# Changelog

All notable changes to BB-Agent will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.0.7] - 2026-04-06

### Fixed

- fullscreen no-auth errors now direct users to `/login`
- successful TUI login now auto-switches to a friendly authenticated model (`gpt-5.4` for OpenAI, `claude-opus-4-6` for Anthropic)
- error and warning notes now use highlighted text without background blocks
- npm launcher and postinstall behavior improved when native binaries are missing

### Added

- Windows release binaries via GitHub Releases
- Windows support in the npm installer/launcher path

### Changed

- release packaging now targets Linux, macOS, and Windows with matching npm/GitHub binary distribution paths

[0.0.7]: https://github.com/shuyhere/bb-agent/releases/tag/v0.0.7
