# Task: add domain error types to crates missing error.rs

Worktree: `/tmp/bb-restructure/r11-errors`
Branch: `r11-add-error-types`

## Goal
7 crates lack dedicated error types and rely on bare `anyhow`: cli, hooks, plugin-host, provider, session, tools, tui.

Add a focused `error.rs` to each with domain-specific error enums using `thiserror`.

## What to do for each crate

For each crate, create `src/error.rs` with a `thiserror`-derived error enum. Scan the crate's source for patterns like:
- `anyhow::bail!("...")`
- `.context("...")`
- `Err(anyhow::anyhow!(...))`
- panic messages

and convert the most common/important failure modes into enum variants.

### Suggested shapes (adapt based on actual code):

**provider**: `ProviderError` — HttpError, StreamError, AuthError, ParseError, UnsupportedModel
**tools**: `ToolError` — ExecutionFailed, InvalidParams, Timeout, NotFound
**session**: `SessionError` — DatabaseError, NotFound, SerializationError, CompactionError
**plugin-host**: `PluginError` — LaunchFailed, ProtocolError, Timeout, NotFound
**hooks**: `HookError` — HandlerFailed, Timeout
**tui**: `TuiError` — RenderError, TerminalError
**cli**: `CliError` — AuthError, ConfigError, SessionError

Each `error.rs` should:
1. `use thiserror::Error;`
2. Define one main error enum
3. Add `pub type Result<T> = std::result::Result<T, TheError>;` if useful
4. NOT convert all anyhow usage — just add the error type file. Gradual migration is fine.

Also update each crate's `lib.rs` (or `main.rs` for cli) to declare `pub mod error;`.

## Important
- Check each crate's `Cargo.toml` — if `thiserror` is not already a dependency, add it.
- Keep error variants concise.
- Do NOT refactor existing code to use the new errors — just add the files.

## Constraints
- Do NOT change behavior.
- Do NOT break existing `anyhow` usage.
- Just ADD the error type files and module declarations.

## Verification
```
cargo build -q
```

## Finish
```
git add -A && git commit -m "add domain error types to all crates"
```
