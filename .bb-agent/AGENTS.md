# BB-Agent Project Context

This is the BB-Agent project — a Rust-native coding agent.

## Project Structure
- `Cargo.toml` — workspace with 8 crates
- `crates/core/` — types, config, error, agent loop
- `crates/session/` — SQLite store, tree, context builder, compaction
- `crates/tools/` — read, bash, edit, write tools + artifact offload
- `crates/provider/` — model registry, OpenAI + Anthropic providers
- `crates/hooks/` — event bus
- `crates/plugin-host/` — TS plugin bridge (JSON-RPC)
- `crates/tui/` — terminal UI (being rebuilt)
- `crates/cli/` — binary entrypoint (`bb`)

## TUI Architecture
We are building a scrollback-based TUI (NOT fullscreen/ratatui).
Uses crossterm directly for terminal ops.
Differential rendering: compare new lines vs previous, only re-render changed lines.
Wrapped in synchronized output (CSI ?2026h/l) for flicker-free updates.

## Key Design
- Component trait: `render(width) -> Vec<String>`, `handle_input(data)`, `invalidate()`
- Container holds children, renders them vertically
- TUI is the root Container with the differential renderer
- Editor component at the bottom for user input
- Overlays render on top of base content

## Build Commands
```
cd ~/BB-Agent
cargo build
cargo test
cargo install --path crates/cli  # installs as `bb`
```

## Important
- Use `crossterm` for terminal ops, NOT ratatui
- Use `unicode-width` for visible_width calculations
- Use `pulldown-cmark` for markdown parsing
- Use `syntect` for code syntax highlighting
- All components must implement the Component trait
- Do NOT modify crates other than `crates/tui/` unless absolutely necessary
