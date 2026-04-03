# Rolling Merge Queue

Last updated: 2026-04-03

## Round 1: `bb-fullscreen`

### Merge now

#### r24-fullscreen-foundation
- Branch: `r24-fullscreen-foundation`
- Commit: `9a2a555`
- Title: `add fullscreen transcript foundation`
- Judgment: ACCEPT
- Build status: passes
- Branch status: clean
- Notes:
  - strongest clean branch in round 1
  - introduces fullscreen path in parallel
  - owns alternate-screen / terminal shell / bottom-fixed input shell / status line shell
  - does not disrupt the old interactive path

#### r25-transcript-block-model
- Branch: `r25-transcript-block-model`
- Commit: `ebbc4c4`
- Title: `add structured transcript block model`
- Judgment: ACCEPT (commit-only)
- Build status: committed state accepted earlier
- Branch status: dirty after accepted commit
- Notes:
  - commit itself is focused and good
  - includes structured transcript blocks, hierarchy, collapse state, and mutation helpers
  - do not take the current dirty branch state after this commit

### Cherry-pick / salvage only

#### r26-projection-scroll
- Branch: `r26-projection-scroll`
- Commit: `6eb6005`
- Title: `add transcript projection and scroll model`
- Judgment: SALVAGE
- Build status: passes
- Branch status: still dirty after commit
- Keep:
  - wrapping
  - row projection mechanics
  - hit-test map ideas
  - viewport auto-follow logic
  - anchor-preservation logic
- Do not merge directly because:
  - it duplicates transcript block types instead of consuming the accepted shared model
  - branch still has extra dirty changes after the useful commit

#### r28-streaming-scheduler
- Branch: `r28-streaming-scheduler`
- Commit: `e442f4f`
- Title: `add streaming scheduler for fullscreen transcript UI`
- Judgment: SALVAGE
- Build status: passes
- Branch status: clean
- Keep:
  - render scheduler idea
  - dirty tracking concepts
  - batching cadence logic
  - idle flush behavior
- Do not merge directly because:
  - implementation is still tied to the old interactive controller / renderer stack
  - not yet integrated into the new shared fullscreen architecture

#### r29-bb-integration
- Branch: `r29-bb-integration`
- Commit: `728b53c`
- Title: `integrate fullscreen transcript UI into bb interactive runtime`
- Judgment: SALVAGE
- Build status: passes
- Branch status: only `Cargo.lock` dirty
- Keep:
  - fullscreen runtime entry wiring
  - runtime event-mapping ideas
  - bottom-fixed fullscreen input flow ideas
- Do not merge directly because:
  - it introduces another CLI-local fullscreen transcript stack
  - it overlaps with the accepted/salvageable responsibilities from `r24`, `r25`, and `r26`

### Reject

#### r27-input-modes-mouse
- Branch: `r27-input-modes-mouse`
- Commit: `be4222f`
- Title: `add transcript mode keyboard and mouse controls`
- Judgment: REJECT
- Build status: passes
- Branch status: clean
- Reason:
  - targets the old interactive controller/component stack rather than the new shared fullscreen path
  - not mergeable into the intended fullscreen architecture without re-porting behavior only

#### Dirty branch states to reject
- `r25` dirty working tree after `ebbc4c4`
- `r26` dirty working tree after `6eb6005`

## Round 2: `bb-fullscreen-next`

### Current purpose
This second round exists to fill the remaining architectural gaps after round 1 by building the shared fullscreen stack properly.

### Reviewed branch states

#### r30-unify-fullscreen-stack
- Branch: `r30-unify-fullscreen-stack`
- Commit status: no commit yet
- Judgment: HOLD
- Build status: passes in current uncommitted state
- Current status:
  - meaningful in-progress work exists
  - touches shared fullscreen stack locations
  - no commit yet, so not mergeable
- Why this is promising:
  - aims at the correct architectural target
  - combines fullscreen foundation with shared transcript state / viewport shell
- Merge readiness:
  - wait for first focused commit

#### r31-projector-viewport-integration
- Branch: `r31-projector-viewport-integration`
- Commit status: no commit yet
- Judgment: HOLD
- Build status: passes in current uncommitted state
- Current status:
  - meaningful in-progress work exists
  - shared fullscreen transcript files are being created under tui-side modules
  - no commit yet, so not mergeable
- Why this is promising:
  - targets the correct architectural layer
  - appears to be integrating model / projector / viewport / wrapping into the shared stack
- Merge readiness:
  - wait for first focused commit

#### r32-transcript-controls
- Branch: `r32-transcript-controls`
- Commit status: no commit yet
- Judgment: REJECT (current state)
- Build status: fails
- Current status:
  - substantial uncommitted work
  - mostly under `crates/cli/src/fullscreen_transcript/`
- Reason:
  - wrong architectural target: building another CLI-local fullscreen subsystem
  - duplicates architecture instead of using the shared fullscreen stack
  - currently broken to build
- Merge readiness:
  - not mergeable in current form

#### r33-fullscreen-streaming
- Branch: `r33-fullscreen-streaming`
- Commit status: no commit yet
- Judgment: SALVAGE / HOLD
- Build status: passes in current uncommitted state
- Current status:
  - focused uncommitted scheduler/streaming work exists
  - still implemented under CLI-local fullscreen transcript modules
- Keep later:
  - scheduler logic
  - batching / idle-flush ideas
  - transcript render cache ideas
- Reason it is not mergeable yet:
  - still targets the wrong architectural layer
  - needs re-port into the shared fullscreen stack

#### r34-fullscreen-runtime-mapping
- Branch: `r34-fullscreen-runtime-mapping`
- Commit status: no commit yet
- Judgment: REJECT (current state)
- Build status: fails
- Current status:
  - partial mixed work between shared fullscreen modules and CLI-local fullscreen transcript modules
  - no coherent clean integration commit yet
- Reason:
  - not yet architecturally coherent
  - currently broken to build
- Merge readiness:
  - not mergeable in current form

## Best current architectural path

### Round 1 accepted base
1. `r24 @ 9a2a555`
2. `r25 @ ebbc4c4` (commit only)

### Salvage from round 1
3. salvage-port from `r26 @ 6eb6005`
4. salvage-port from `r28 @ e442f4f`
5. salvage-port from `r29 @ 728b53c`

### Most promising active round-2 branches
6. wait for first focused commit from `r30`
7. wait for first focused commit from `r31`

### Branches to ignore unless restarted cleanly
8. `r27`
9. current `r32`
10. current `r34`

## Preferred merge order

1. `r24 @ 9a2a555`
2. `r25 @ ebbc4c4`
3. salvage-port projector / viewport ideas from `r26`
4. salvage-port scheduler ideas from `r28`
5. salvage-port runtime wiring from `r29`
6. focused follow-up commits from round 2 in this preferred order:
   - `r30`
   - `r31`
   - `r33`

## Live monitoring

### Round 1
- tmux session: `bb-fullscreen`
- progress log: `/tmp/bb-fullscreen/monitor.log`
- commit review log: `/tmp/bb-fullscreen/review-events.log`

### Round 2
- tmux session: `bb-fullscreen-next`
- progress log: `/tmp/bb-fullscreen-next/monitor.log`
- commit review log: `/tmp/bb-fullscreen-next/review-events.log`
