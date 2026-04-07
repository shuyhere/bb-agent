# BB-Agent

![BB-Agent title figure](assets/title-figure.png)

> BB means Bridge Baby in Death Stranding. I named this project that way because while building it, I was also enjoying Death Stranding and loved the idea of connecting everyone together.

A Rust-native AI coding agent for the terminal — featuring a fullscreen TUI, multi-provider support, tool use, session persistence, branching, extensions, and skills.

## Install

### Terminal & Font Compatibility

BB-Agent uses Unicode glyphs and ANSI color in the fullscreen TUI. For the best visual experience, use a modern terminal and a Unicode-capable monospace font such as:

- JetBrains Mono
- SF Mono / Menlo
- Fira Code
- Cascadia Mono
- Nerd Font variants of the above

If some symbols look broken, missing, or too narrow in your terminal:

1. switch to a Unicode-capable monospace font
2. make sure your terminal uses UTF-8
3. enable BB-Agent compatibility mode

Compatibility mode uses safer ASCII-style fallback glyphs for spinner/status/tool markers:

```bash
BB_TUI_COMPAT=1 bb
```

Or set this in `~/.bb-agent/settings.json`:

```json
{
  "compatibility_mode": true
}
```


### From source (all platforms — macOS, Linux, Windows)

Requires [Rust](https://rustup.rs). Install Rust first if you don't have it:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env
```

Then build and install BB-Agent:

```bash
git clone https://github.com/shuyhere/bb-agent.git
cd bb-agent
cargo install --path crates/cli
```

This compiles the `bb` binary and installs it to `~/.cargo/bin/bb` (which Rust adds to your PATH).

### npm (Linux/macOS/Windows — downloads matching prebuilt binary when available)

```bash
npm install -g @shuyhere/bb-agent
```

> If no matching prebuilt binary is available for your platform, npm install will print source-build instructions instead. After install, run `bb` to start.
>
> Current GitHub release binaries are published for Linux x86_64, macOS x86_64/arm64, and Windows x86_64.

## Getting Started

### 1. Start the TUI

```bash
bb
```

That's the recommended way to get started.

### 2. Log in with `/login`

Inside the TUI, run:

```text
/login
```

This opens the provider picker and auth flow directly in the fullscreen UI.

If you prefer, you can also log in from a normal terminal:

```bash
bb login              # Interactive provider selection
bb login anthropic    # Login to Anthropic (OAuth)
bb login openai-codex # Login to OpenAI (OAuth)
bb login google       # Login to Google (API key)
```

Or set environment variables: `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `GOOGLE_API_KEY`, etc.

That's it! Run `bb` to launch the fullscreen interactive terminal UI. Type your prompt and press Enter.

### More ways to use `bb`

```bash
bb                                    # Launch the fullscreen TUI
bb "Explain this codebase"            # TUI with an initial prompt
bb -p "What is 2+2?"                  # Print mode (non-interactive, pipe-friendly)
bb -c                                 # Continue your last session
bb -r                                 # Resume: pick from previous sessions
bb --model sonnet                     # Use a specific model
bb --model sonnet:high                # Model with extended thinking
bb --list-models                      # List all available models
```

## Features

- **Fullscreen TUI** — rich terminal interface with streaming output, markdown rendering, syntax highlighting
- **Multi-provider** — Anthropic (Claude), OpenAI, Google (Gemini), Groq, xAI, OpenRouter, and custom OpenAI-compatible endpoints
- **Built-in tools** — `read`, `write`, `edit`, `bash`, `find`, `grep`, `ls`, `web_search`, `web_fetch`, `browser_fetch`
- **Session persistence** — SQLite-backed sessions with branching, forking, and tree navigation
- **Extensions** — JS/TS plugin system for custom tools, commands, and hooks
- **Skills** — markdown-based instruction files that auto-load contextual knowledge
- **System prompt templates** — save and switch between named prompt configurations
- **OAuth login** — browser/device login for Anthropic, OpenAI, and GitHub Copilot

## System Prompt Templates

Save prompt templates in `~/.bb-agent/system-prompts/` and switch between them:

```bash
bb --list-templates                   # List available templates
bb -t coding                          # Start with "coding" template
bb -t research                        # Start with "research" template
bb --system-prompt @path/to/file.md   # Load prompt from any file
```

## Extensions & Skills

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

## Keyboard Shortcuts

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

## Workspace Crates

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

## Documentation

- [Configuration Reference](docs/configuration.md) — settings.json, AGENTS.md, templates
- [Built-in Tools](docs/tools.md) — all 10 tools with parameters
- [Extensions & Skills](docs/extensions.md) — plugins, skills, prompts, packages
- [Providers & Models](docs/providers.md) — authentication, model selection, custom providers
- [Development Guide](docs/development.md) — build from source, dev workflow, project structure, debugging
- [Contributing](CONTRIBUTING.md) — code style, PR process
- [Changelog](CHANGELOG.md) — release history
- [Security](SECURITY.md) — vulnerability reporting, security model

## Development

See the full [Development Guide](docs/development.md) for detailed instructions.

```bash
git clone https://github.com/shuyhere/bb-agent.git
cd bb-agent
cargo install --path crates/cli      # Build + install to ~/.cargo/bin/bb
bb                                   # Run it
```

Dev cycle:
```bash
cargo run --bin bb                   # Run without installing
cargo test --workspace --release     # Run all 435 tests
cargo fmt --all                      # Format
cargo clippy --workspace             # Lint
```

## License

[MIT License](LICENSE)
