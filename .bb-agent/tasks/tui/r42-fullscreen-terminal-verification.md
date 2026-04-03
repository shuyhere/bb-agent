# Task: r42 fullscreen terminal verification and polish

Worktree: `/tmp/bb-fullscreen-finish/r42-verify`
Branch: `r42-fullscreen-terminal-verification`

## Goal

Do the first real verification/polish pass for the shared fullscreen transcript path after the accepted integration plus streaming/runtime work are available on the branch.

## Starting point

Base from current `master` and first integrate the accepted fullscreen base:
1. cherry-pick `c990227`
2. cherry-pick `e5796f5`

If needed for verification, you may also selectively port minimal stable pieces from:
- `fcb193c`
- `5402cfb`

But do not build a second architecture.

## Required focus

### 1. Terminal-behavior checks
Verify and tighten:
- resize behavior
- scrolling behavior
- auto-follow on/off behavior
- click toggle behavior
- cursor/input behavior by mode
- no obvious flicker regressions

### 2. Lightweight targeted polish only
Allowed:
- small shared fullscreen runtime fixes
- small frame/render fixes
- small fullscreen entry fixes
- small tests if helpful

Not allowed:
- large new architecture
- duplicate fullscreen subsystem
- broad unrelated refactors

### 3. Keep architecture boundary clean
Allowed targets:
- `crates/tui/src/fullscreen/*`
- `crates/cli/src/fullscreen_entry.rs`
- minimal CLI entry plumbing only if required

Forbidden targets:
- `crates/cli/src/interactive_fullscreen/*`
- `crates/cli/src/fullscreen_transcript/*`

## Verification

```bash
cd /tmp/bb-fullscreen-finish/r42-verify
cargo build
cargo test -p bb-tui fullscreen::runtime -- --nocapture
cargo run -p bb-cli -- --help | rg "fullscreen|transcript"
```

## Finish

```bash
git add -A && git commit -m "polish shared fullscreen terminal behavior"
```
