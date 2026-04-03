# Task: r28 streaming scheduler and incremental rendering

Worktree: `/tmp/bb-fullscreen/r28-streaming`
Branch: `r28-streaming-scheduler`

## Goal

Make the new fullscreen transcript UI stable during streaming.
This branch owns per-block streaming updates, dirty tracking, render scheduling, and no-flicker incremental redraw behavior.

## Main deliverables

### 1. Per-block streaming
Support a streaming target block id and append incoming content directly into that block.

Examples:
- assistant text streams into active assistant content block
- thinking streams into thinking block
- tool progress streams into tool use or tool result block

### 2. Dirty tracking
Track which blocks changed and avoid rebuilding unrelated transcript state.

### 3. Render scheduler
Add a render scheduler that:

- marks state dirty on events
- batches rapid token updates
- optionally caps frame frequency
- flushes frames on a timer or event boundary

### 4. Incremental rendering
Update only what changed in the fullscreen view.
Do not clear the whole screen for routine streaming updates.

### 5. Stability while user scrolls
When auto-follow is disabled because the user is reading history:

- still ingest streaming updates
- do not yank viewport to bottom
- do not break focused row logic

## Suggested files

- `runtime/streaming.rs`
- `runtime/scheduler.rs`
- `render/frame.rs`
- `render/mod.rs`

## Required tests

Add tests for:

- streaming append preserves prior content
- dirty block tracking is selective
- scheduler batches rapid updates
- auto-follow false prevents viewport jumps during streaming

## Constraints

- No full-screen clear during routine token updates.
- Preserve good behavior for long sessions.
- Keep scheduler logic separate from transcript data structures.

## Verification

```bash
cd /tmp/bb-fullscreen/r28-streaming
cargo test
cargo build
```

## Finish

```bash
git add -A && git commit -m "add streaming scheduler for fullscreen transcript UI"
```
