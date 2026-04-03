# Task: r38 fullscreen cleanup and cutover prep

Worktree: `/tmp/bb-fullscreen-final/r38-cleanup`
Branch: `r38-fullscreen-cleanup`

## Goal

Clean up duplicate fullscreen experiments and make the shared fullscreen path ready for final integration after the controls/streaming/runtime jobs land.

## Current problem

There are multiple experimental fullscreen implementations across old branch worktrees. The final solution must be the shared fullscreen path on `master`, not the duplicate CLI-local fullscreen transcript trees seen in earlier branches.

## Main deliverables

### 1. Clean shared exports and module boundaries
Ensure:
- `crates/tui/src/fullscreen/` exports are clean
- no duplicate fullscreen module definitions remain
- public API is small and coherent

### 2. Remove or avoid duplicate fullscreen architecture
If any duplicate fullscreen transcript path exists in the branch worktree, do not preserve it.
Prepare the tree for the shared fullscreen solution only.

### 3. Tighten entry flags / naming
Ensure the fullscreen entry switch naming is coherent and documented in code.
Keep backward-compatible aliases if necessary.

### 4. Build stability
Keep `cargo build` passing while the shared fullscreen path is present alongside the old interactive mode.

## Constraints

- Do not rip out the old interactive mode yet.
- Do not add new functionality that belongs in the controls/streaming/runtime jobs.
- Focus on cleanup and prep for final merge.

## Verification

```bash
cd /tmp/bb-fullscreen-final/r38-cleanup
cargo build
```

## Finish

```bash
git add -A && git commit -m "clean up shared fullscreen integration surface"
```
