# R50: Interactive UI Notification/Status Consumption

## Goal
Wire the `InteractiveUiHandler` captured notifications and statuses into the
interactive controller's render loop so that extension `ctx.ui.notify()` and
`ctx.ui.setStatus()` calls produce visible feedback in the TUI.

## Scope
- After every extension command execution or event dispatch in interactive mode,
  drain `InteractiveUiHandler::drain_notifications()` and show them via
  `self.show_status()` or `self.add_chat_message(InteractiveMessage::System { ... })`.
- After every extension event/command, read `get_statuses()` and display them
  in the footer via `rebuild_footer()` or similar surface.
- Do NOT implement real TUI widget select/confirm/input dialogs — just surface
  fire-and-forget feedback.

## Files to Touch
- `crates/cli/src/interactive/controller/submission_flow.rs` — after `dispatch_prompt`
  and extension command execution, drain notifications.
- `crates/cli/src/interactive/controller/command_actions.rs` — after reload, drain
  notifications from the handler.
- `crates/cli/src/extensions.rs` — expose a getter to retrieve the `InteractiveUiHandler`
  from `ExtensionCommandRegistry` (needs downcast or typed accessor).

## Constraints
- Keep changes in `crates/cli/` only (no core changes needed).
- Use existing `show_status` / `add_chat_message` methods.
- Must compile with `cargo build -q -p bb-cli`.
- Must pass `cargo test -q -p bb-cli`.
