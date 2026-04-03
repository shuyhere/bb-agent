# Task: r36 shared fullscreen streaming

Worktree: `/tmp/bb-fullscreen-final/r36-streaming`
Branch: `r36-shared-fullscreen-streaming`

## Goal

Add batched streaming, dirty tracking, and no-flicker update scheduling to the shared fullscreen path on `master`.

## Current base state

`master` already has:
- fullscreen foundation
- shared transcript model
- shared projection
- shared viewport

What is still missing is a proper shared fullscreen streaming scheduler and incremental update path.

## Implement in the shared fullscreen path only

Target files under:
- `crates/tui/src/fullscreen/`
- `crates/cli/src/interactive_fullscreen/`

Do NOT implement this in the old interactive controller path.
Do NOT create another CLI-local fullscreen transcript subsystem.

## Required features

### 1. Shared render scheduler
Add:
- dirty flag
- frame cadence cap
- idle flush behavior
- batched flush during token bursts

### 2. Per-block dirty updates
Use the shared transcript block model.
Support incremental updates for:
- assistant content
- thinking
- tool use
- tool result

### 3. Auto-follow interaction
When the user scrolls away from the bottom:
- continue ingesting stream updates
- do not force-jump the viewport back to the bottom

### 4. Shared fullscreen redraw behavior
Keep redraw incremental and no-flicker.
Do not full-clear on routine streaming updates.

## Reuse / port from references only
References:
- salvage branch `r28-streaming-scheduler`
- salvage/hold branch `r33-fullscreen-streaming`
- shared fullscreen code on `master`

Port ideas only, not the old or duplicate architecture.

## Verification

```bash
cd /tmp/bb-fullscreen-final/r36-streaming
cargo build
```

## Finish

```bash
git add -A && git commit -m "add shared fullscreen streaming scheduler"
```
