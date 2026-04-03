# Task: r33 fullscreen streaming scheduler

Worktree: `/tmp/bb-fullscreen-next/r33-streaming`
Branch: `r33-fullscreen-streaming`

## Goal

Implement the fullscreen streaming update path using per-block dirty tracking and a render scheduler.

## Main deliverables

### 1. Per-block streaming
Support streaming append into a target transcript block.
At minimum cover:
- assistant content
- thinking
- tool use / tool result updates

### 2. Dirty tracking
Track which blocks changed.
Avoid rebuilding unrelated transcript state on every token.

### 3. Render scheduler
Add:
- dirty flag
- frame cadence cap
- batched flush during token bursts
- flush-on-idle behavior

### 4. Fullscreen incremental redraw
Use the fullscreen path only.
Do not full-clear during routine token updates.

### 5. Auto-follow interaction
If the user scrolls away, continue ingesting stream updates but do not force jump to bottom.

## Required sources

- conceptual reference: current dirty `r28-streaming-scheduler` branch
- shared fullscreen transcript stack from the new jobs

## Constraints

- Implement against the fullscreen path.
- Keep scheduler logic separate from transcript domain types.
- No repo-wide unrelated rewrites.

## Verification

```bash
cd /tmp/bb-fullscreen-next/r33-streaming
cargo build
cargo test
```

## Finish

```bash
git add -A && git commit -m "add fullscreen transcript streaming scheduler"
```
