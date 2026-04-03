# Task: r37 shared fullscreen runtime mapping

Worktree: `/tmp/bb-fullscreen-final/r37-runtime`
Branch: `r37-shared-fullscreen-runtime-mapping`

## Goal

Wire BB-Agent runtime events into the shared fullscreen transcript stack on `master`.

## Current base state

`master` already has:
- fullscreen foundation
- shared structured transcript model
- shared projection
- shared viewport
- local prompt capture in fullscreen mode

What is still missing is real runtime integration.

## Implement in the shared fullscreen path only

Target files under:
- `crates/cli/src/interactive_fullscreen/`
- `crates/tui/src/fullscreen/`
- minimal entry wiring in `crates/cli/src/interactive.rs` / `main.rs` only if required

Do NOT create another `crates/cli/src/fullscreen_transcript/` subsystem.
Do NOT modify the old interactive controller unless absolutely necessary for shared runtime plumbing.

## Required event mapping
Map at least:
- user messages
- assistant turn start/end
- assistant text deltas
- thinking deltas
- tool call start/args/executing/result
- status / warning / error notes

## Required hierarchy
Use the shared transcript block model to represent:
- assistant turn
  - thinking
  - tool use
    - tool result
  - assistant content

## Required behavior
- bottom-fixed editor still works
- fullscreen mode can submit prompts and display real streamed results
- old interactive mode remains buildable

## Reuse / port from references only
References:
- salvage branch `r29-bb-integration`
- current `master`

Port behavior only, not duplicate architecture.

## Verification

```bash
cd /tmp/bb-fullscreen-final/r37-runtime
cargo build
```

## Finish

```bash
git add -A && git commit -m "map bb runtime events into shared fullscreen transcript"
```
