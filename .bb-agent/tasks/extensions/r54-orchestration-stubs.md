# R54: Wire Orchestration TODO Stubs

## Goal
Replace the remaining TODO stubs in `crates/core/src/agent_session/orchestration.rs`
with real implementations that connect to the already-built extension infrastructure.

## Scope
The file has three TODOs:
1. `TODO: hook concrete runtime agent events.` — Wire the session/hook bus so
   that `session_start` / `session_shutdown` events are emitted at session
   lifecycle boundaries.
2. `TODO: port runtime tool / extension initialization.` — Use the
   `SessionResourceBootstrap` that's already threaded through config/state to
   populate active tools from both built-in and extension-registered tools.
3. `TODO: integrate extension command execution.` — When a prompt is detected
   as an extension command (via `is_registered_extension_command`), route it
   through the extension command execution path instead of queueing it as a
   regular prompt.

## Files to Touch
- `crates/core/src/agent_session/orchestration.rs` — replace TODOs.
- `crates/core/src/agent_session/session.rs` — may need to expose helpers.
- `crates/core/src/agent_session_runtime/host.rs` — may need accessors.

## Constraints
- Must compile with `cargo build -q -p bb-core`.
- Must pass `cargo test -q -p bb-core`.
- Do NOT break existing orchestration tests.
- Keep changes focused on wiring; do not restructure the orchestration module.
