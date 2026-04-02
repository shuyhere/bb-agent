# Task: split `crates/core/src/agent_loop.rs`

Worktree: `/tmp/bb-restructure/r06-agent-loop`
Branch: `r06-split-agent-loop`

## Goal
`agent_loop.rs` is 793 lines with 8 structs + 16 fns mixing loop mechanics, event types, and tool execution.

Split into a module tree at `crates/core/src/agent_loop/`.

## Principles
- `mod.rs` routing only
- One file, one responsibility
- Types separate from behavior

## Likely split
Read the file and split:

1. `types.rs` — AgentLoopEvent, LoopAssistantMessage, ToolCallInfo, and other data structs/enums
2. `tool_execution.rs` — prepare/execute/finalize tool call helpers, sequential/parallel execution
3. `streaming.rs` — stream_assistant_response and related helpers
4. `runner.rs` — agent_loop, agent_loop_continue, run_agent_loop, run_agent_loop_continue, run_loop (the main loop orchestration)
5. `mod.rs` — routing + re-exports

## Constraints
- Do NOT redesign behavior.
- Do NOT rename public types.
- Touch other files ONLY if needed for imports.
- `crates/core/src/lib.rs` declares `pub mod agent_loop;` — this will automatically pick up the module directory.

## Verification
```
cargo build -q
cargo test -q -p bb-core
```

## Finish
```
git add -A
git commit -m "split agent_loop into module tree by responsibility"
```

Report: changed files, verification results, commit hash.
