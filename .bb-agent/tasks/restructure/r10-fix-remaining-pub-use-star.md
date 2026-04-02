# Task: replace remaining `pub use *` with explicit re-exports

Worktree: `/tmp/bb-restructure/r10-pub-star`
Branch: `r10-fix-remaining-pub-use-star`

## Goal
7 remaining `pub use module::*` patterns:

In `crates/core/src/types/mod.rs`:
- `pub use content::*;`
- `pub use messages::*;`
- `pub use session::*;`

In `crates/core/src/agent_session_extensions/mod.rs`:
- `pub use types::*;`
- `pub use resources::*;`
- `pub use models::*;`
- `pub use runner::*;`

Replace each with explicit named re-exports.

## Method
1. For each glob, grep the workspace to find which names are actually imported externally.
2. Replace the glob with `pub use module::{Name1, Name2, ...};`
3. If unsure, read the child module and list all `pub` items, then re-export them explicitly.

## Constraints
- Do NOT change type definitions.
- Do NOT remove types used externally.
- Preserve full workspace compilation.

## Verification
```
cargo build -q
cargo test -q -p bb-core
```

## Finish
```
git add -A && git commit -m "replace remaining pub use * with explicit re-exports"
```
