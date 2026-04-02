# Task: move logic out of lib.rs files

Worktree: `/tmp/bb-restructure/r15-lib-rs`
Branch: `r15-clean-lib-rs`

## Goal
Two `lib.rs` files contain logic (fn/impl) instead of being pure routing:

1. `crates/provider/src/lib.rs` (79 lines) — contains CompletionRequest, RequestOptions, StreamEvent, UsageInfo structs + Provider trait
2. `crates/tools/src/lib.rs` (60 lines) — contains ToolResult, ToolContext, Tool trait + builtin_tools() fn

Move types and traits to dedicated files. `lib.rs` should only declare modules and re-export.

## Method

### For `crates/provider/src/lib.rs`:
- Move CompletionRequest, RequestOptions, StreamEvent, UsageInfo → `types.rs`
- Move Provider trait → `traits.rs` (or keep in `types.rs`)
- `lib.rs` becomes: module declarations + `pub use types::*;` or explicit re-exports

### For `crates/tools/src/lib.rs`:
- Move ToolResult, ToolContext, Tool trait → `types.rs`
- Move builtin_tools() → `registry.rs` or keep inline (it's tiny)
- `lib.rs` becomes: module declarations + explicit re-exports

## Constraints
- Do NOT change type definitions.
- Preserve all public API.
- Prefer explicit re-exports over `pub use *`.

## Verification
```
cargo build -q
```

## Finish
```
git add -A && git commit -m "move logic out of lib.rs into dedicated type files"
```
