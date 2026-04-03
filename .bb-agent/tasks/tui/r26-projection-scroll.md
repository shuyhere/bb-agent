# Task: r26 projection and scroll model

Worktree: `/tmp/bb-fullscreen/r26-projection-scroll`
Branch: `r26-projection-scroll`

## Goal

Implement the projection layer that converts structured transcript blocks into visible rows for the fullscreen transcript viewport.

This branch owns scroll behavior, wrapping, visible row generation, and anchor preservation.

## Main deliverables

### 1. Projection layer
Create a projection system that:

- flattens only visible transcript content into rows
- hides children when a block is collapsed
- wraps text for the current viewport width
- emits action rows such as `* Thinking`
- records row-to-block mappings
- records hit-test metadata for clickable headers

### 2. Scroll state
Add a viewport model with:

- `viewport_top`
- `viewport_height`
- optional focused row / selected block
- total projected row count

### 3. Auto-follow behavior
Implement:

- default `auto_follow = true`
- scrolling upward disables auto-follow
- jumping back to bottom re-enables auto-follow
- new streamed content only moves viewport when auto-follow is enabled

### 4. Anchor preservation
When expanding/collapsing a visible block or when resizing, preserve local reading position.
Recommended rule:

- if a visible header is toggled, keep that header at or near the same row after projection update

### 5. Caching
Add projection caches for:

- wrapped block lines
- block visible height
- projected row fragments

Invalidate only on:
- width change
- content change
- collapse-state change

## Suggested files

- `projection.rs`
- `wrapping.rs`
- `viewport.rs`
- `hit_test.rs`

## Required tests

Add tests for:

- collapsed block hides children
- expanded block shows content rows
- wrapping changes with width
- hit-test rows map to block ids
- auto-follow toggles correctly
- anchor preservation keeps header near same row after expand/collapse

## Constraints

- Single scroll model only.
- No nested scroll regions.
- Do not force bottom-follow while user is reading history.

## Verification

```bash
cd /tmp/bb-fullscreen/r26-projection-scroll
cargo test
cargo build
```

## Finish

```bash
git add -A && git commit -m "add transcript projection and scroll model"
```
