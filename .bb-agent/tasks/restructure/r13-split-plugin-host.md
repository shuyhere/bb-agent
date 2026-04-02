# Task: split `crates/plugin-host/src/host.rs` (523 lines)

Worktree: `/tmp/bb-restructure/r13-plugin-host`
Branch: `r13-split-plugin-host`

## Goal
`host.rs` mixes protocol handling, lifecycle management, and messaging in one file.

Split into a module tree.

## Likely split
Read the file and separate:

1. `types.rs` — data structs, message types, config types
2. `lifecycle.rs` — plugin startup, shutdown, health check
3. `messaging.rs` — message send/receive, JSON-RPC protocol handling
4. `host.rs` — the main PluginHost struct orchestration (kept small)
5. Update `lib.rs` to route through the module

## Constraints
- Do NOT change behavior.
- Keep `lib.rs` clean.
- Preserve public API.

## Verification
```
cargo build -q -p bb-plugin-host
```

## Finish
```
git add -A && git commit -m "split plugin-host/host.rs by responsibility"
```
