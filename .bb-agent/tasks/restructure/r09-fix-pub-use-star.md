# Task: replace `pub use *` with explicit re-exports

Worktree: `/tmp/bb-restructure/r09-pub-use-star`
Branch: `r09-fix-pub-use-star`

## Goal
Three places use `pub use module::*` which leaks internal types:

1. `crates/core/src/lib.rs` — `pub use agent_session::*;`
2. `crates/core/src/agent_session_runtime/mod.rs` — `pub use types::*;`
3. `crates/hooks/src/lib.rs` — `pub use events::*;`

Replace each with explicit `pub use` of only the types that are actually used externally.

## Method
For each glob re-export:

1. Run `rg 'bb_core::agent_session::' crates/ --type rust` (or equivalent) to find which types are actually imported elsewhere.
2. Replace the glob with explicit named re-exports covering exactly those types.
3. If some types are only used internally within the crate, keep them as `pub(crate)` or remove the re-export.

## Constraints
- Do NOT change any type definitions.
- Do NOT remove types that are used externally.
- Preserve compilation of the entire workspace.
- This is a SMALL task — just three `pub use *` lines to replace.

## Verification
```
cargo build -q
cargo test -q -p bb-core -p bb-hooks
```

## Finish
```
git add -A
git commit -m "replace pub use * with explicit re-exports"
```

Report: changed files, verification results, commit hash.
