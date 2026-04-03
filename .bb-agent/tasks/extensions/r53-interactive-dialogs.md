# R53: Interactive UI Dialog Methods

## Goal
Make extension dialog methods (`ctx.ui.select()`, `ctx.ui.confirm()`,
`ctx.ui.input()`) produce real TUI interactions in interactive mode instead
of returning defaults.

## Scope
- Upgrade `InteractiveUiHandler` in `crates/cli/src/extensions.rs`:
  - `confirm`: Use a oneshot channel. The handler stores the request and
    signals the interactive controller. The controller shows a yes/no prompt
    (simple status bar or chat message with key binding). When the user
    responds, resolve the channel.
  - `select`: Similar — show options in status/chat, resolve on keypress.
  - `input`: Show a prompt in the editor area, resolve when user submits.
  - For all: implement a timeout fallback that resolves with the default
    if the request includes a `timeout` field.

## Architecture
- `InteractiveUiHandler` holds a `tokio::sync::mpsc::Sender<PendingUiDialog>`.
- The interactive controller holds the corresponding receiver and checks it
  in the event loop.
- When a dialog arrives, the controller enters a special mode (like auth mode)
  that redirects editor submission to resolve the dialog.
- This is analogous to how `pending_auth_provider` works in `submission_flow.rs`.

## Files to Touch
- `crates/cli/src/extensions.rs` — add channel plumbing to `InteractiveUiHandler`.
- `crates/cli/src/interactive/controller/submission_flow.rs` — handle pending dialog.
- `crates/cli/src/interactive/controller/mod.rs` — add dialog state field.

## Constraints
- Keep changes in `crates/cli/` only.
- Must compile with `cargo build -q -p bb-cli`.
- Must pass `cargo test -q -p bb-cli`.
- This is the most complex task. If time-constrained, implement `confirm` only
  and leave `select`/`input` returning defaults with a TODO.
