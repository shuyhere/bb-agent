# Rolling Merge Queue

Last updated: 2026-04-03

This queue now reflects the latest reviewed state after the final fullscreen subagent round.

## Current `master` base

Already on `master`:
- `8d5af47` `add fullscreen transcript foundation`
- `18c071e` `add structured transcript block model`
- `c360a71` `unify fullscreen foundation with structured transcript state`
- `f7dbd86` `launch final fullscreen tui subagent round`

## Merge now

### r38-fullscreen-cleanup
- Branch: `r38-fullscreen-cleanup`
- Commit: `c990227`
- Title: `clean up shared fullscreen integration surface`
- Judgment: ACCEPT
- Build status: passes
- Extra verification: `cargo run -p bb-cli -- --help` shows canonical `--fullscreen-transcript` and legacy alias `--fullscreen`
- Why merge now:
  - removes the obsolete duplicate fullscreen integration surface
  - adds thin `crates/cli/src/fullscreen_entry.rs`
  - keeps the shared fullscreen stack as the only target architecture

### r35-shared-fullscreen-controls
- Branch: `r35-shared-fullscreen-controls`
- Commit: `e5796f5`
- Title: `add transcript controls to shared fullscreen runtime`
- Judgment: ACCEPT
- Build status: passes
- Test status:
  - `cargo test -p bb-tui fullscreen::runtime -- --nocapture` passes
- Why merge now:
  - adds transcript mode controls to the shared fullscreen runtime
  - adds focused block styling, mouse toggle, wheel scroll, and search scaffold
  - the important work is in the shared TUI layer
- Merge note:
  - merge after `r38`
  - expect a small conflict because `r35` still touched a file that `r38` deletes
  - keep the shared fullscreen runtime/frame changes, drop stale CLI-local path references

## Salvage next

### r36-shared-fullscreen-streaming
- Branch: `r36-shared-fullscreen-streaming`
- Commit: `fcb193c`
- Title: `add shared fullscreen streaming scheduler`
- Judgment: SALVAGE
- Build status: passes
- Test status:
  - `cargo test -p bb-tui fullscreen::runtime -- --nocapture` passes
- Keep:
  - `crates/tui/src/fullscreen/scheduler.rs`
  - dirty tracking concepts
  - frame cadence cap
  - batching / idle flush logic
  - shared runtime/projection improvements that do not depend on obsolete CLI-local fullscreen files
- Do not merge directly because:
  - the branch still uses the obsolete CLI-local fullscreen integration surface
  - the final architecture must route through the thin shared entry instead

### r37-shared-fullscreen-runtime-mapping
- Branch: `r37-shared-fullscreen-runtime-mapping`
- Commit: `5402cfb`
- Title: `map bb runtime events into shared fullscreen transcript`
- Judgment: SALVAGE
- Build status: passes
- Test status:
  - build passes
  - no meaningful targeted fullscreen-runtime test coverage landed with the commit itself
- Keep:
  - runtime-event to transcript-block mapping ideas
  - user / assistant / thinking / tool / status hierarchy wiring ideas
  - prompt submission and streaming integration ideas
- Do not merge directly because:
  - the main implementation still lives on the obsolete CLI-local fullscreen integration surface
  - the logic must be re-homed into `crates/cli/src/fullscreen_entry.rs` and shared fullscreen APIs

## Preferred next integration order

1. merge `c990227` from `r38`
2. merge `e5796f5` from `r35`
3. salvage-port streaming from `fcb193c` into the shared fullscreen stack
4. salvage-port runtime mapping from `5402cfb` into the shared fullscreen stack
5. run attached-terminal verification and long-session streaming checks

## Explicit accept / salvage / reject state

### ACCEPT
- `r38-fullscreen-cleanup @ c990227`
- `r35-shared-fullscreen-controls @ e5796f5`

### SALVAGE
- `r36-shared-fullscreen-streaming @ fcb193c`
- `r37-shared-fullscreen-runtime-mapping @ 5402cfb`

### REJECT AS A FINAL TARGET SURFACE
- any reintroduction of `crates/cli/src/interactive_fullscreen/*`
- any new `crates/cli/src/fullscreen_transcript/*` stack
- any fullscreen implementation that duplicates the shared `crates/tui/src/fullscreen/*` stack instead of extending it

## Working rule for remaining TUI work

All remaining fullscreen work must follow this boundary:

- shared fullscreen behavior belongs in `crates/tui/src/fullscreen/*`
- BB-specific wiring belongs in `crates/cli/src/fullscreen_entry.rs`
- no second fullscreen architecture is allowed to grow beside that shared stack
