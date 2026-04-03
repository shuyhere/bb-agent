# Rolling Merge Queue

Last updated: 2026-04-03

## Round 1: `bb-fullscreen`

### Merge now

#### r24-fullscreen-foundation
- Branch: `r24-fullscreen-foundation`
- Commit: `9a2a555`
- Title: `add fullscreen transcript foundation`
- Judgment: ACCEPT
- Notes:
  - clean branch
  - fullscreen shell is isolated
  - keeps old interactive mode intact

#### r25-transcript-block-model
- Branch: `r25-transcript-block-model`
- Commit: `ebbc4c4`
- Title: `add structured transcript block model`
- Judgment: ACCEPT (commit-only)
- Notes:
  - commit is focused and good
  - do not take current dirty branch state after this commit

### Cherry-pick / salvage only

#### r26-projection-scroll
- Branch: `r26-projection-scroll`
- Commit: `6eb6005`
- Title: `add transcript projection and scroll model`
- Judgment: SALVAGE
- Keep:
  - wrapping
  - projection mechanics
  - hit-test map
  - viewport auto-follow / anchor-preservation ideas
- Do not merge directly because:
  - it duplicates transcript block types instead of consuming the accepted model
  - branch has extra dirty changes after the commit

#### r29-bb-integration
- Branch: `r29-bb-integration`
- Commit: `728b53c`
- Title: `integrate fullscreen transcript UI into bb interactive runtime`
- Judgment: SALVAGE
- Keep:
  - entry wiring
  - runtime event-mapping ideas
  - bottom-fixed fullscreen input flow ideas
- Do not merge directly because:
  - it introduces another CLI-local fullscreen transcript stack
  - it overlaps with `r24`, `r25`, and `r26`

### Cherry-pick / salvage only

#### r28-streaming-scheduler
- Branch: `r28-streaming-scheduler`
- Commit: `e442f4f`
- Title: `add streaming scheduler for fullscreen transcript UI`
- Judgment: SALVAGE
- Keep:
  - render scheduler idea
  - dirty tracking concepts
  - batching cadence logic
- Do not merge directly because:
  - implementation is still tied to the old interactive controller and renderer
  - branch state remains broader than the target shared fullscreen stack

### Reject

#### r27-input-modes-mouse
- Branch: `r27-input-modes-mouse`
- Commit: `be4222f`
- Title: `add transcript mode keyboard and mouse controls`
- Judgment: REJECT
- Reason:
  - builds, but targets the old interactive controller/component stack instead of the new shared fullscreen path
  - not mergeable into the intended fullscreen architecture without re-porting

#### Dirty branch states to reject
- `r25` dirty working tree after `ebbc4c4`
- `r26` dirty working tree after `6eb6005`
- extra dirty state after `e442f4f` on `r28`

## Round 2: `bb-fullscreen-next`

### Current purpose
This second round exists to fill the gaps left after round 1.

### Active jobs
- `r30-unify-fullscreen-stack`
- `r31-projector-viewport-integration`
- `r32-transcript-controls`
- `r33-fullscreen-streaming`
- `r34-fullscreen-runtime-mapping`

### Queue state
- No reviewed commits yet in round 2 at the time of this update.
- These branches are under active monitoring and commit-arrival review.
- The round-2 monitor and review windows are live in tmux.

## Target merge order

### Preferred integration order
1. `r24 @ 9a2a555`
2. `r25 @ ebbc4c4`
3. salvage-port from `r26`
4. salvage-port from `r29`
5. focused follow-up commits from round 2 in this order:
   - `r30`
   - `r31`
   - `r32`
   - `r33`
   - `r34`

## Live monitoring

### Round 1
- tmux session: `bb-fullscreen`
- progress log: `/tmp/bb-fullscreen/monitor.log`
- commit review log: `/tmp/bb-fullscreen/review-events.log`

### Round 2
- tmux session: `bb-fullscreen-next`
- progress log: `/tmp/bb-fullscreen-next/monitor.log`
- commit review log: `/tmp/bb-fullscreen-next/review-events.log`
