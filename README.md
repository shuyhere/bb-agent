# BB-Agent

A Rust-native AI coding agent for the terminal — featuring a fullscreen TUI, multi-provider support, tool use, session persistence, branching, extensions, and skills.

## Features

- **Fullscreen TUI** — rich terminal interface with streaming output, markdown rendering, syntax highlighting
- **Multi-provider** — Anthropic (Claude), OpenAI, Google (Gemini), Groq, xAI, OpenRouter, and custom OpenAI-compatible endpoints
- **Built-in tools** — `read`, `write`, `edit`, `bash`, `find`, `grep`, `ls`, `web_search`, `web_fetch`, `browser_fetch`
- **Session persistence** — SQLite-backed sessions with branching, forking, and tree navigation
- **Extensions** — JS/TS plugin system for custom tools, commands, and hooks
- **Skills** — markdown-based instruction files that auto-load contextual knowledge
- **System prompt templates** — save and switch between named prompt configurations
- **OAuth login** — browser-based login for Anthropic and OpenAI

## Quick start

### Build from source

```bash
git clone https://github.com/shuyhere/bb-agent.git
cd bb-agent
cargo install --path crates/cli
```

### Login

```bash
bb login              # Interactive provider selection
bb login anthropic    # Login to Anthropic (OAuth)
bb login openai-codex # Login to OpenAI (OAuth)
bb login google       # Login to Google (API key)
```

Or set environment variables: `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `GOOGLE_API_KEY`, etc.

### Usage

```bash
bb                                    # Fullscreen interactive mode
bb "Explain this codebase"            # Start with an initial prompt
bb -p "What is 2+2?"                  # Print mode (non-interactive)
bb -c                                 # Continue previous session
bb -r                                 # Resume: pick a session
bb --model sonnet                     # Use a specific model
bb --model anthropic/claude-sonnet-4-20250514:high  # Model with thinking
bb --list-models                      # List available models
```

### System prompt templates

```bash
# Save templates in ~/.bb-agent/system-prompts/
bb --list-templates                   # List available templates
bb -t coding                          # Start with "coding" template
bb -t research                        # Start with "research" template
bb --system-prompt @path/to/file.md   # Load prompt from file
```

### Extensions & Skills

```bash
bb install npm:some-skill             # Install a global package
bb install --local ./my-skill         # Install project-local
bb list                               # List installed packages
bb update                             # Update packages
```

## Configuration

BB-Agent uses layered configuration:

| File | Scope |
|------|-------|
| `~/.bb-agent/settings.json` | Global settings |
| `<project>/.bb-agent/settings.json` | Project settings (overrides global) |
| `~/.bb-agent/AGENTS.md` or `AGENTS.md` | System prompt additions |
| `~/.bb-agent/system-prompts/<name>.md` | Named prompt templates |
| `~/.bb-agent/skills/` | Global skills |
| `~/.bb-agent/extensions/` | Global extensions |

### Example `settings.json`

```json
{
  "default_model": "claude-sonnet-4-20250514",
  "default_provider": "anthropic",
  "default_thinking": "medium",
  "models": [
    {
      "id": "my-local-model",
      "provider": "ollama",
      "api": "openai",
      "base_url": "http://localhost:11434/v1",
      "context_window": 32000,
      "max_tokens": 4096
    }
  ]
}
```

## Keyboard shortcuts

| Key | Action |
|-----|--------|
| `Enter` | Submit prompt |
| `Shift+Enter` | Insert newline |
| `Esc` | Clear input / cancel / exit prompt |
| `Ctrl+C` | Exit |
| `Ctrl+P` | Cycle models |
| `Ctrl+O` | Open settings menu |
| `Ctrl+Shift+O` | Expand/collapse tool calls |
| `/` | Slash commands menu |
| `@` | File mention autocomplete |

## Workspace crates

| Crate | Description |
|-------|-------------|
| `bb-core` | Core agent, session, config, and runtime types |
| `bb-session` | SQLite-backed session storage, branching, context building |
| `bb-tools` | Built-in tool implementations |
| `bb-provider` | Model/provider integrations and streaming |
| `bb-hooks` | Hook event types for extensions |
| `bb-plugin-host` | Plugin discovery and host runtime |
| `bb-tui` | Terminal UI components and fullscreen experience |
| `bb-cli` | The `bb` command-line application |

## Development

```bash
cargo build --workspace              # Build all
cargo test --workspace --release     # Run all tests
cargo fmt --all                      # Format
cargo clippy --workspace             # Lint
```

## Documentation

- [Configuration Reference](docs/configuration.md) — settings.json, AGENTS.md, templates
- [Built-in Tools](docs/tools.md) — all 10 tools with parameters
- [Extensions & Skills](docs/extensions.md) — plugins, skills, prompts, packages
- [Providers & Models](docs/providers.md) — authentication, model selection, custom providers
- [Contributing](CONTRIBUTING.md) — development setup, code style, PR process
- [Changelog](CHANGELOG.md) — release history
- [Security](SECURITY.md) — vulnerability reporting, security model

## License

Licensed under either of:

- [MIT License](LICENSE-MIT)
- [Apache License, Version 2.0](LICENSE-APACHE)

at your option.
