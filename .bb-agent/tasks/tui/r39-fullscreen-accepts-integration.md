# Task: r39 fullscreen accepted-work integration

Worktree: `/tmp/bb-fullscreen-finish/r39-integration`
Branch: `r39-fullscreen-accepts-integration`

## Goal

Integrate the two already-reviewed ACCEPT commits into one clean branch and leave the repo on the correct shared fullscreen architecture.

## Accepted source commits

- `c990227` from `r38-fullscreen-cleanup`
- `e5796f5` from `r35-shared-fullscreen-controls`

## Required work

### 1. Start from current `master`
Use current `master` as base.

### 2. Integrate accepted work in this order
1. cherry-pick `c990227`
2. cherry-pick `e5796f5`

Resolve conflicts carefully.

### 3. Keep only the correct architecture
After conflict resolution, keep this boundary:
- fullscreen shared behavior in `crates/tui/src/fullscreen/*`
- thin BB adapter in `crates/cli/src/fullscreen_entry.rs`
- no surviving `crates/cli/src/interactive_fullscreen/*`
- no new duplicate fullscreen subtree

### 4. Preserve the accepted functionality
The integrated branch must retain:
- thin fullscreen entry surface
- `--fullscreen-transcript` canonical flag
- `--fullscreen` alias
- transcript controls in the shared fullscreen runtime
- focused header styling
- search scaffold
- wheel scrolling and click toggle

## Verification

```bash
cd /tmp/bb-fullscreen-finish/r39-integration
cargo build
cargo test -p bb-tui fullscreen::runtime -- --nocapture
cargo run -p bb-cli -- --help | rg "fullscreen|transcript"
```

## Finish

```bash
git add -A && git commit -m "integrate accepted shared fullscreen cleanup and controls"
```
