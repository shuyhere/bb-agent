# Task: decompose InteractiveMode god struct (49 fields)

Worktree: `/tmp/bb-restructure/r05-interactive-mode`
Branch: `r05-interactive-mode-decompose`

## Goal
`InteractiveMode` in `crates/cli/src/interactive/controller/mode.rs` has 49 fields. This violates:
- Principle 5 (separate orchestration from implementation)
- Principle 10 (make illegal states hard to represent)
- Principle 14 (prefer composition)

Extract field groups into composed structs so InteractiveMode becomes an orchestrator that holds smaller state objects.

## Target files
- `crates/cli/src/interactive/controller/mode.rs` — main target
- Other files in `crates/cli/src/interactive/controller/` — will need import updates

## Suggested decomposition
Read the struct fields carefully and group them:

1. **`UIContainers`** — all the `Arc<Mutex<Container>>` fields (header, chat, pending, status, widget_above, widget_below, footer) + editor + ui (TUI)
2. **`StreamingState`** — streaming_text, streaming_thinking, streaming_tool_calls, is_streaming, pending_working_message, status_loader, hide_thinking_block, hidden_thinking_label
3. **`QueueState`** — steering_queue, follow_up_queue, compaction_queued_messages, pending_bash_components
4. **`RenderCache`** — header_lines, chat_lines, pending_lines, status_lines, footer_lines, widgets_above_lines, widgets_below_lines
5. **`InteractionState`** — last_sigint_time, last_escape_time, is_bash_running, is_bash_mode, is_compacting, shutdown_requested, is_initialized, tool_output_expanded

Then `InteractiveMode` holds these composed structs + the remaining core fields (controller, session_setup, options, version, etc).

## Important
- Put the new structs in the SAME file or in new sibling files under `controller/`.
- Update all `self.field` accesses to `self.ui.field` or `self.streaming.field` etc.
- The controller/ directory already has: agent_events.rs, command_actions.rs, editor_lifecycle.rs, interaction_controls.rs, key_actions.rs, model_actions.rs, rendering.rs, runtime.rs, shared.rs, submission_flow.rs, ui_state.rs
- ALL of these files use `super::mode::InteractiveMode` and access its fields via `self.some_field`.
- You MUST update field access paths in ALL those files.

## Constraints
- Do NOT change behavior.
- Do NOT add features.
- Preserve all `pub(super)` visibility.
- Keep `mod.rs` as routing only.

## Verification
```
cargo build -q
cargo test -q -p bb-cli
```

## Finish
```
git add -A
git commit -m "decompose InteractiveMode into composed state structs"
```

Report: changed files, verification results, commit hash.
