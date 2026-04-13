# Development Guide

This guide walks you through setting up BB-Agent for local development — building from source, running in dev mode, making changes, and testing.

## Prerequisites

- **Rust 1.93+** (nightly features are used)
- **Git**
- **A C compiler** (`gcc` or `clang`) — needed for SQLite bundled build
- **Node.js 16+** (optional, only for testing extensions)
- **Chrome/Chromium** (optional, only for the `browser_fetch` tool)

### Install Rust

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

The project includes a `rust-toolchain.toml` that pins the exact Rust version. Rustup will automatically install it when you first build.

## Clone & Build

```bash
git clone https://github.com/shuyhere/bb-agent.git
cd bb-agent
cargo install --path crates/cli
```

This compiles the optimized `bb` binary and installs it to `~/.cargo/bin/bb`.
Rust adds `~/.cargo/bin` to your PATH during installation.

Verify:

```bash
bb --version
bb
```

> **Tip:** `cargo install --path crates/cli` is equivalent to `cargo build --release` + copying `target/release/bb` to your PATH. Use `cargo build --release` during development when you don't want to overwrite your installed version.

## Dev Mode Workflow

### Quick iteration cycle

During development, use these commands:

```bash
# Build (debug mode — faster compile, slower runtime)
cargo build

# Run directly without installing
cargo run --bin bb

# Run with arguments
cargo run --bin bb -- "hello world"
cargo run --bin bb -- --help
cargo run --bin bb -- -p "What is 2+2?"

# Build release (optimized — slower compile, fast runtime)
cargo build --release

# Install your local build into PATH
cargo install --path crates/cli
```

### Run from debug build

If you don't want to install, run directly:

```bash
./target/debug/bb
./target/release/bb
```

### Rebuild on change

For rapid iteration, use `cargo watch` (install with `cargo install cargo-watch`):

```bash
# Rebuild on every file change
cargo watch -x "build --bin bb"

# Rebuild and run tests
cargo watch -x "test --workspace --release"
```

## Testing

```bash
# Run all tests (use --release to avoid debug linker issues on Linux)
cargo test --workspace --release

# Run tests for a specific crate
cargo test -p bb-core --release
cargo test -p bb-tui --release
cargo test -p bb-tools --release
cargo test -p bb-session --release
cargo test -p bb-provider --release
cargo test -p bb-cli --release

# Run a specific test
cargo test -p bb-tui --release -- fullscreen::tests::typing_at

# Run tests with output
cargo test --workspace --release -- --nocapture
```

## Linting & Formatting

```bash
# Format all code
cargo fmt --all

# Check formatting (CI-friendly)
cargo fmt --all -- --check

# Run clippy (zero warnings required)
cargo clippy --workspace --all-targets

# Strict mode (treat warnings as errors)
cargo clippy --workspace --all-targets -- -D warnings
```

## Project Structure

```
bb-agent/
├── Cargo.toml              # Workspace root
├── rust-toolchain.toml     # Pinned Rust version (1.93.0)
├── .cargo/config.toml      # Linker config (Linux)
├── crates/
│   ├── core/               # bb-core: agent, session, config, runtime types
│   │   └── src/
│   │       ├── agent/          # Agent runtime, callbacks, events
│   │       ├── agent_session/  # Session config, models, orchestration
│   │       ├── agent_session_runtime/  # Runtime host, compaction, retry
│   │       ├── config.rs       # Global/project directory resolution
│   │       ├── settings.rs     # Settings struct + layered merge
│   │       ├── types/          # ContentBlock, SessionEntry, etc.
│   │       └── error.rs        # BbError enum
│   │
│   ├── session/            # bb-session: SQLite persistence
│   │   └── src/
│   │       ├── store/          # CRUD, queries, fork
│   │       ├── schema.rs       # DB schema + migrations
│   │       ├── tree.rs         # Session tree/branching
│   │       ├── context/        # Context assembly for LLM
│   │       └── compaction/     # Token-aware compaction
│   │
│   ├── tools/              # bb-tools: built-in tool implementations
│   │   └── src/
│   │       ├── bash.rs         # Shell execution
│   │       ├── read.rs         # File reading (text + images)
│   │       ├── write.rs        # File writing
│   │       ├── edit.rs         # Precise text replacement
│   │       ├── find.rs, grep.rs, ls.rs  # File search tools
│   │       ├── web_search/     # DuckDuckGo search
│   │       ├── web_fetch/      # HTTP fetch + HTML extraction
│   │       ├── browser_fetch/  # Headless Chrome fetch
│   │       └── registry.rs     # builtin_tools() list
│   │
│   ├── provider/           # bb-provider: LLM API integrations
│   │   └── src/
│   │       ├── anthropic.rs    # Anthropic (Claude) streaming
│   │       ├── openai.rs       # OpenAI + compatible APIs
│   │       ├── google.rs       # Google Gemini
│   │       ├── registry/       # Built-in model definitions
│   │       ├── retry.rs        # Exponential backoff
│   │       └── transforms.rs   # Message format conversion
│   │
│   ├── hooks/              # bb-hooks: extension event types
│   │   └── src/
│   │       ├── events.rs       # Event enum (Input, ToolCall, etc.)
│   │       └── bus.rs          # Async event bus
│   │
│   ├── plugin-host/        # bb-plugin-host: JS/TS extension runtime
│   │   └── src/
│   │       ├── host/           # Plugin lifecycle, messaging, UI
│   │       ├── discovery.rs    # Find plugins on disk
│   │       └── protocol.rs     # JSON stdin/stdout protocol
│   │
│   ├── tui/                # bb-tui: terminal UI (largest crate)
│   │   └── src/
│   │       ├── fullscreen/     # Legacy-named TUI app/runtime (events, frame, projection, etc.)
│   │       ├── editor/         # Multi-line text editor
│   │       ├── markdown/       # Markdown rendering
│   │       ├── syntax.rs       # Syntax highlighting
│   │       ├── select_list.rs  # Fuzzy select menu
│   │       └── theme.rs        # Terminal color themes
│   │
│   └── cli/                # bb-cli: the `bb` binary
│       └── src/
│           ├── main.rs         # CLI arg parsing, entry point
│           ├── fullscreen/     # Legacy-named TUI controller, menus, session
│           ├── session_bootstrap.rs  # Runtime setup
│           ├── turn_runner.rs  # LLM turn execution
│           ├── login.rs        # Auth store (auth.json)
│           ├── oauth/          # OAuth PKCE flows
│           ├── extensions.rs   # Extension discovery + loading
│           └── slash.rs        # Slash command dispatch
│
├── bin/bb                  # npm launcher (Node.js shim)
├── scripts/postinstall.js  # npm postinstall (binary download)
├── package.json            # npm package metadata
└── docs/                   # Documentation
```

## Key Code Paths

### What happens when you type `bb`

1. `cli/src/main.rs` → parses CLI args
2. `cli/src/session_bootstrap.rs` → loads settings, resolves model/provider, builds tools
3. `cli/src/fullscreen/mod.rs` → creates TUI config, spawns controller + UI tasks
4. `tui/src/fullscreen/` → runs the terminal event loop (key/mouse/paste → state → render)
5. `cli/src/fullscreen/controller/loop_impl.rs` → processes submissions, runs LLM turns
6. `cli/src/turn_runner/runner.rs` → sends messages to provider, executes tool calls

### What happens when the agent calls a tool

1. Provider streams a `tool_use` event
2. `turn_runner/tools.rs` → finds the tool, calls `tool.execute(params, ctx, cancel)`
3. e.g. `tools/src/edit.rs` → reads file, applies edits, writes file, returns diff
4. Result is appended to conversation and sent back to the LLM

### Adding a new built-in tool

1. Create `crates/tools/src/my_tool.rs`
2. Implement the `Tool` trait:
   ```rust
   #[async_trait]
   impl Tool for MyTool {
       fn name(&self) -> &str { "my_tool" }
       fn description(&self) -> &str { "..." }
       fn parameters_schema(&self) -> Value { json!({...}) }
       async fn execute(&self, params: Value, ctx: &ToolContext, cancel: CancellationToken) -> BbResult<ToolResult> {
           // ...
       }
   }
   ```
3. Add `pub mod my_tool;` to `crates/tools/src/lib.rs`
4. Register in `crates/tools/src/registry.rs`:
   ```rust
   Box::new(crate::my_tool::MyTool),
   ```
5. Add tool description to `DEFAULT_SYSTEM_PROMPT` in `crates/core/src/agent/helpers.rs`

### Adding a new provider

1. Create `crates/provider/src/my_provider.rs`
2. Implement the `Provider` trait (see `crates/provider/src/traits.rs`)
3. Add models to `crates/provider/src/registry/models/`
4. Wire the provider selection in `crates/cli/src/session_bootstrap.rs` (the `match model.api` block)

## Debugging

### Enable tracing

```bash
# Show all log levels
bb --verbose

# Or set env var
RUST_LOG=debug bb
RUST_LOG=bb_core=trace,bb_provider=debug bb
```

### Debug a specific crate

```bash
# Run only bb-tui tests with output
cargo test -p bb-tui --release -- --nocapture

# Run with backtrace on panic
RUST_BACKTRACE=1 cargo run --bin bb
```

### Inspect the session database

```bash
sqlite3 ~/.bb-agent/sessions.db
.tables
SELECT session_id, cwd, created_at FROM sessions ORDER BY created_at DESC LIMIT 5;
SELECT entry_id, type, substr(payload, 1, 100) FROM entries WHERE session_id = '...' ORDER BY seq;
```

## Release Checklist

```bash
# 1. Ensure everything is clean
cargo fmt --all -- --check
cargo clippy --workspace --all-targets
cargo test --workspace --release

# 2. Update version in Cargo.toml (workspace.package.version)

# 3. Update CHANGELOG.md

# 4. Commit, tag, push
git add -A
git commit -m "v0.x.x: description"
git tag -a v0.x.x -m "v0.x.x"
git push origin master --tags

# 5. Build release binary
cargo build --release

# 6. Upload binary to GitHub release
gh release create v0.x.x target/release/bb --title "v0.x.x"

# 7. Update package.json version, publish to npm
npm publish
```
