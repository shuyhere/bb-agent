# Changelog

All notable changes to BB-Agent will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.0.5] - 2026-04-06

### Added

- **Fullscreen TUI** with streaming output, markdown rendering, and syntax highlighting
- **Multi-provider support**: Anthropic (Claude), OpenAI, Google (Gemini), Groq, xAI, OpenRouter, and custom OpenAI-compatible endpoints
- **Built-in tools**: `read`, `write`, `edit`, `bash`, `find`, `grep`, `ls`, `web_search`, `web_fetch`, `browser_fetch`
- **Session persistence** with SQLite-backed storage, branching, forking, and tree navigation
- **Extensions** via JS/TS plugin system for custom tools, commands, and event hooks
- **Skills** — markdown-based instruction files that auto-load contextual knowledge
- **System prompt templates** — save named prompts in `~/.bb-agent/system-prompts/` and use with `bb -t <name>`
- **OAuth login** for Anthropic and OpenAI (browser-based PKCE flow)
- **`@` file mention** autocomplete in the input area
- **`/` slash commands** for session management, model switching, and more
- **Layered configuration** — global `~/.bb-agent/settings.json` merged with project `.bb-agent/settings.json`
- **`AGENTS.md`** support (like Claude's `CLAUDE.md`) for persistent system prompt additions
- **Custom models and providers** via settings.json
- **Auto-retry** with exponential backoff and server-hinted delays
- **Session compaction** to keep context within model limits
- **Package management** — install skills/extensions from npm, git, or local paths
- **Print mode** (`bb -p`) for non-interactive scripted usage
- **Session resume** (`bb -c` to continue, `bb -r` to pick)

[0.0.5]: https://github.com/shuyhere/bb-agent/releases/tag/v0.0.5
