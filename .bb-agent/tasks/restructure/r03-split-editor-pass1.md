# Task: split `crates/tui/src/editor.rs` (pass 1)

Worktree: `/tmp/bb-restructure/r03-editor-pass1`
Branch: `r03-split-editor-pass1`

## Goal
Do the first safe restructuring pass on `crates/tui/src/editor.rs` so it starts matching the repo principles:
- `mod.rs` is routing/re-export only
- one file, one responsibility
- separate editor state/types, rendering/layout, and input/editing behavior
- preserve behavior and current tests

## Scope
Primary target:
- `crates/tui/src/editor.rs`

Expected result for pass 1:
- replace the monolithic file with `crates/tui/src/editor/`
- keep public `Editor` API working
- perform a safe first split, not a speculative redesign

## Important
This file is large and behavior-sensitive. Do a conservative split.
If needed, prefer a first pass like:
- `types.rs` for editor state/snapshots/layout/helper enums
- `rendering.rs` for layout/render helpers and `Component` rendering impl
- `input.rs` / `editing.rs` for editing/navigation/history/select/undo behavior
- `mod.rs` only routing + re-exports

Do not change UX or key behavior beyond what is needed for the restructure.

## Constraints
- No feature redesign.
- No ratatui/fullscreen changes.
- Preserve existing tests.
- Keep `mod.rs` thin.
- Touch other files only if required for imports.
- Follow the same restructuring discipline already used elsewhere in the repo.

## References
- pi source shape: `/home/shuyhere/tmp/pi-mono/packages/tui/src/components/editor.ts`
- BB references:
  - `crates/core/src/agent_session_runtime/mod.rs`
  - `crates/cli/src/interactive/controller/mod.rs`
  - `crates/tui/src/components/mod.rs`

## Verification
Run:
- `cargo build -q`
- `cargo test -q -p bb-tui`

## Finish
Commit on your branch with:
- `git commit -am "split editor module pass 1 by responsibility"`
  - include new files with `git add` as needed

In your final output, report:
- changed files
- verification commands run
- commit hash
