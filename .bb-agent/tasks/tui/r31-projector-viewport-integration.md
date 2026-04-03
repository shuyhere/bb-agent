# Task: r31 projector and viewport integration

Worktree: `/tmp/bb-fullscreen-next/r31-projector`
Branch: `r31-projector-viewport-integration`

## Goal

Port the good projection/viewport work into the unified fullscreen stack, but make it consume the accepted structured transcript block model rather than introducing duplicate types.

## Required sources

- transcript model reference: `r25-transcript-block-model` commit `ebbc4c4`
- projector salvage reference: `r26-projection-scroll` commit `6eb6005`

## Main deliverables

### 1. Projection over shared transcript blocks
Implement:
- wrapping
- visible row projection
- header/content row kinds
- row-to-block mapping
- hit-test map

Use the shared fullscreen transcript block model, not the ad-hoc `TranscriptBlock` from `r26`.

### 2. Viewport state
Implement:
- viewport top/height
- total projected rows
- auto-follow
- anchor capture/preserve on expand/collapse and resize
- row/block lookup helpers

### 3. Cache policy
Keep wrapping/projection caches focused and explicit.
Invalidate on:
- width change
- content change
- collapse-state change

### 4. Export clean API
Expose reusable projector + viewport APIs that later jobs can consume.

## Constraints

- No CLI-local transcript types.
- No scrollback-based old interactive assumptions.
- Single transcript viewport only.

## Verification

```bash
cd /tmp/bb-fullscreen-next/r31-projector
cargo test
cargo build
```

## Finish

```bash
git add -A && git commit -m "integrate projector and viewport with fullscreen transcript blocks"
```
