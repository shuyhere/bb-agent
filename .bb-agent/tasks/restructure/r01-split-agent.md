# Task: split `crates/core/src/agent.rs`

Worktree: `/tmp/bb-restructure/r01-agent`
Branch: `r01-split-agent`

## Goal
Restructure `crates/core/src/agent.rs` into a focused module tree that matches the repo principles:
- `mod.rs` is routing/re-export only
- one file, one responsibility
- separate public API, internal state, orchestration, and helpers
- small intentional public facade
- preserve behavior and existing public API shape as much as possible

## Scope
Primary target:
- `crates/core/src/agent.rs`

Expected result:
- replace the monolithic file with `crates/core/src/agent/`
- keep `crates/core/src/lib.rs` public surface working
- preserve current semantics and call sites

## Likely split points
Use the code itself as the source of truth, but a good split will likely separate:
- public data/config/event types
- message/tool/text extraction helpers
- abort controller/signal types
- queue management
- agent state types
- runtime/orchestration implementation for `Agent`

## Constraints
- Do not redesign behavior.
- Do not add new features.
- Keep re-exports clean.
- Keep `mod.rs` thin.
- Touch other files only if required to preserve imports/exports.
- Follow the existing restructure style already used in:
  - `crates/core/src/agent_session_runtime/`
  - `crates/cli/src/interactive/controller/`
  - `crates/session/src/compaction/`

## References
- pi source shape: `/home/shuyhere/tmp/pi-mono/packages/agent/src/agent.ts`
- related BB files:
  - `crates/core/src/agent_loop.rs`
  - `crates/core/src/agent_session.rs`
  - `crates/core/src/agent_session_runtime/mod.rs`

## Verification
Run:
- `cargo build -q`
- `cargo test -q -p bb-core`

## Finish
Commit on your branch with:
- `git commit -am "split agent module by responsibility"`
  - include new files with `git add` as needed

In your final output, report:
- changed files
- verification commands run
- commit hash
