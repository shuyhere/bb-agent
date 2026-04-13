# Contributing to BB-Agent

Thank you for your interest in contributing! Here's how to get started.

For full setup instructions, project structure, and debugging tips, see the [Development Guide](docs/development.md).

## Quick Start

```bash
# Install Rust (if needed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env

# Clone, build, install
git clone https://github.com/shuyhere/bb-agent.git
cd bb-agent
cargo install --path crates/cli

# Verify
bb --version
bb
```

### Prerequisites

- **Rust 1.93+** (pinned in `rust-toolchain.toml` — auto-installed by rustup)
- A C compiler (`cc` / `gcc` / `clang`) for SQLite and linking
- Optional: Chrome/Chromium for `browser_fetch` tool
- Optional: Node.js 16+ for testing extensions

## Making Changes

1. **Fork** the repository
2. **Create a branch** from `master`: `git checkout -b my-feature`
3. **Make your changes**
4. **Ensure quality**:
   ```bash
   cargo fmt --all
   cargo clippy --workspace --all-targets
   cargo test --workspace --release
   ```
5. **Commit** with a clear message
6. **Open a Pull Request**

## Code Style

- Run `cargo fmt --all` before committing
- Run `cargo clippy --workspace --all-targets` — zero warnings required
- Write tests for new functionality
- Use `thiserror` for error types, `anyhow` for application-level errors
- Doc comments (`///`) on all public items
- No `unsafe` code
- No `unwrap()`/`expect()` outside of tests and static initialization

## Project Structure

```
crates/
├── core/          # Core agent, session, config, runtime types
├── session/       # SQLite session storage, branching, compaction
├── tools/         # Built-in tool implementations
├── provider/      # LLM provider integrations (Anthropic, OpenAI, Google, etc.)
├── hooks/         # Event types for extensions
├── plugin-host/   # JS/TS plugin discovery and runtime
├── tui/           # Terminal UI components and tui experience
└── cli/           # The `bb` binary — CLI, controller, TUI wiring
```

## Adding a New Tool

1. Create a new module in `crates/tools/src/`
2. Implement the `Tool` trait (see `crates/tools/src/types.rs`)
3. Register it in `crates/tools/src/registry.rs`
4. Add tests

## Adding a New Provider

1. Create a new module in `crates/provider/src/`
2. Implement the `Provider` trait (see `crates/provider/src/traits.rs`)
3. Add models to `crates/provider/src/registry/models/`
4. Wire it in `crates/cli/src/session_bootstrap.rs`

## Adding a Skill

Skills are just markdown files. Create `~/.bb-agent/skills/<name>/SKILL.md`:

```markdown
---
name: my-skill
description: What this skill does
---
Instructions for the agent...
```

## Reporting Issues

- Use [GitHub Issues](https://github.com/shuyhere/bb-agent/issues)
- Include: OS, Rust version (`rustc --version`), BB-Agent version (`bb --version`)
- For bugs: steps to reproduce, expected vs actual behavior

## License

By contributing, you agree that your contributions will be licensed under the [MIT License](LICENSE).
