# Fullscreen Transcript UI Gap Review

Current accepted/salvageable inputs:

- `r24-fullscreen-foundation` @ `9a2a555` — accepted foundation
- `r25-transcript-block-model` @ `ebbc4c4` — accepted transcript block model commit
- `r26-projection-scroll` @ `6eb6005` — salvage projection/viewport/hit-test/wrapping ideas
- `r29-bb-integration` @ `728b53c` — salvage event-mapping and fullscreen runtime ideas

Current non-mergeable or off-scope inputs:

- `r25` dirty working tree after `ebbc4c4`
- `r26` dirty working tree after `6eb6005`
- `r27-input-modes-mouse` current branch state
- `r28-streaming-scheduler` current dirty branch state

## What is already covered

### Foundation
Covered by `r24`:
- alternate screen ownership
- mouse capture shell
- fullscreen frame shell
- bottom-fixed input area
- status line area
- dedicated fullscreen entry path

### Structured transcript model
Covered by `r25` commit:
- block ids
- block kinds
- parent / child relationships
- collapse state
- mutation helpers for streamed content

### Projection ideas
Covered conceptually by `r26` commit:
- wrapping
- row projection
- hit-test map
- viewport state
- auto-follow / anchor-preservation logic

### BB runtime integration direction
Covered conceptually by `r29` commit:
- new fullscreen runtime path
- mapping runtime events into transcript-like UI state

## What is NOT implemented yet

### 1. No unified stack
The accepted pieces are still split across branches and duplicate each other.
There is not yet one shared fullscreen implementation that combines:
- `r24` foundation
- `r25` transcript blocks
- `r26` projection/viewport
- `r29` event mapping

### 2. Foundation still uses plain transcript items
`r24` foundation currently uses a simple transcript item list rather than the structured transcript block tree from `r25`.

### 3. Projection is not wired to the accepted block model
`r26` introduced its own transcript block type instead of consuming the transcript model from `r25`.

### 4. Transcript mode is not implemented
The plan requires:
- `Ctrl+O` transcript mode toggle
- normal / transcript / search modes
- focused row state
- transcript navigation keys

This is not completed in the new fullscreen path.

### 5. Expand/collapse interaction is not finished
Missing in the unified fullscreen path:
- focused block toggling
- click-to-toggle using hit testing
- visible focused styling for action rows
- search-mode navigation scaffold

### 6. Streaming scheduler is not integrated
Missing in the fullscreen path:
- per-block dirty tracking
- batched render scheduler
- incremental redraw tied to transcript blocks
- anti-flicker batching during token bursts

### 7. BB runtime event mapping is not unified with the shared stack
`r29` maps events into a separate CLI-local transcript implementation.
That must be rewritten to target the shared fullscreen transcript model.

### 8. Auto-follow and anchor preservation are not wired end-to-end
The ideas exist in `r26`, but they are not yet integrated into the accepted fullscreen path.

### 9. Visible-only rendering / performance path is incomplete
The plan calls for:
- visible projection fragments
- cache invalidation on width/content/collapse change
- stable long-session performance

This is only partially present in salvage code and not yet unified.

## New subagent job list

1. unify fullscreen stack
2. wire projection + viewport onto structured transcript blocks
3. implement transcript mode / controls / mouse toggles
4. add fullscreen streaming scheduler and dirty rendering
5. map BB runtime events into the shared fullscreen transcript stack
