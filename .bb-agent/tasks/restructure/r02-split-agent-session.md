# Task: split `crates/core/src/agent_session.rs`

Worktree: `/tmp/bb-restructure/r02-agent-session`
Branch: `r02-split-agent-session`

## Goal
Restructure `crates/core/src/agent_session.rs` into a focused module tree that matches the repo principles:
- `mod.rs` is routing/re-export only
- one file, one responsibility
- separate public API from internal implementation/state
- separate conversion/helpers from orchestration
- preserve behavior and public API as much as possible

## Scope
Primary target:
- `crates/core/src/agent_session.rs`

Expected result:
- replace the monolithic file with `crates/core/src/agent_session/`
- keep existing public imports working from `bb_core::agent_session` / `bb_core::*`
- keep behavior stable

## Likely split points
Use the file contents as the source of truth, but likely separations include:
- session public types/config/events/models/tool definitions
- thin print session / print-mode helpers
- provider/message conversion helpers
- AGENTS.md loading and model parsing helpers
- `AgentSession` implementation/orchestration
- error types

## Constraints
- No behavior redesign.
- No new features.
- Keep `mod.rs` thin.
- Keep public surface intentional and stable.
- Touch other files only when required for imports/re-exports.
- Follow the same restructuring style as:
  - `crates/core/src/agent_session_runtime/`
  - `crates/cli/src/interactive/`
  - `crates/session/src/compaction/`

## References
- pi source shape: `/home/shuyhere/tmp/pi-mono/packages/coding-agent/src/core/agent-session.ts`
- related BB files:
  - `crates/core/src/agent.rs`
  - `crates/core/src/agent_session_runtime/mod.rs`
  - `crates/core/src/lib.rs`

## Verification
Run:
- `cargo build -q`
- `cargo test -q -p bb-core`

## Finish
Commit on your branch with:
- `git commit -am "split agent_session module by responsibility"`
  - include new files with `git add` as needed

In your final output, report:
- changed files
- verification commands run
- commit hash
