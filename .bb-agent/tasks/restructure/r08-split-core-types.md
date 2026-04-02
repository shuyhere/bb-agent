# Task: split `crates/core/src/types.rs` (21 structs)

Worktree: `/tmp/bb-restructure/r08-core-types`
Branch: `r08-split-core-types`

## Goal
`core/types.rs` is 368 lines with 21 structs dumped in one file — a type dumping ground.

Split into a module tree at `crates/core/src/types/`.

## Principles
- One file, one responsibility
- `mod.rs` routing only
- Group by domain, not "all types"

## Likely split
Read the file and group by domain:

1. `messages.rs` — AgentMessage enum and its variants (User, Assistant, ToolResult, CompactionSummary, BranchSummary, etc), MessageRole
2. `content.rs` — ContentBlock, AssistantContent, ToolCall content types
3. `session.rs` — SessionEntry, EntryBase, EntryId, SessionId and related session data types
4. `mod.rs` — routing + re-exports preserving the existing `bb_core::types::*` public surface

## Important
- `crates/core/src/lib.rs` has `pub mod types;` — the directory will be picked up automatically.
- MANY files across the ENTIRE workspace import from `bb_core::types::` — you must preserve the same public names.
- Run `rg 'bb_core::types::' crates/` to see all import sites. The re-exports in mod.rs must cover every type currently used externally.

## Constraints
- Do NOT rename any types.
- Do NOT change any type definitions.
- Preserve ALL existing public imports.
- Touch other files ONLY if absolutely needed (ideally zero changes outside `crates/core/src/types/`).

## Verification
```
cargo build -q
cargo test -q -p bb-core
```

## Finish
```
git add -A
git commit -m "split core types.rs into domain-grouped module tree"
```

Report: changed files, verification results, commit hash.
