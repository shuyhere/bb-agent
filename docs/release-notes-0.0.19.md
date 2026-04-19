# Release notes draft — v0.0.19

_Date: 2026-04-19_

## Highlights

### Bash timeout/cancel is now a real hard stop
This release fixes a nasty bash-tool failure mode where BB could still appear hung after timeout or cancel.

Previously, BB would kill the timed-out or cancelled bash process, but could still block forever while draining stdout/stderr if a detached/background child inherited the pipe and kept it open.

`v0.0.19` replaces that unbounded post-kill drain with a bounded drain on timeout/cancel, while preserving full draining for normal exits.

The result is that bash timeout/cancel now behaves much more like users expect: once BB decides the command is over, it actually stops.

### Esc now prioritizes cancelling active work
This release also fixes the TUI Esc-precedence bug.

When a turn/tool/local action was cancellable, `Esc` could still get consumed by local UI behavior first:
- clear input
- jump back to the bottom of the transcript
- leave transcript mode

Now, when there is cancellable work in flight, `Esc` requests cancellation first.

That makes long-running local actions feel much more predictable in the TUI.

### Click-to-expand hints and toggling are restored/polished
The TUI once again explicitly tells users that clicking works for tool expansion:
- `Click or Ctrl+Shift+O to expand`
- `Click or Ctrl+Shift+O to collapse`

The mouse toggle path is also more robust when the clicked row belongs to wrapped or nested transcript content, and old persisted click-help hint text is still handled cleanly.

## Notable user-facing changes

### Changed
- tool expand/collapse hints now explicitly mention mouse clicking again
- mouse toggling for transcript tool blocks is more robust across wrapped/nested rows

### Fixed
- bash timeout/cancel no longer hangs after kill while waiting for inherited stdout/stderr pipes to reach EOF
- `Esc` now requests cancellation before input clear, scroll reset, or transcript-mode exit can consume the key
- persisted old click-help hint text remains visually compatible with the current TUI rendering path

### Improved
- regression coverage now includes detached-child bash timeout hangs and focused TUI Esc-priority interaction cases

## Upgrade notes
- No manual migration is required.
- Existing sessions/transcripts continue to load normally.
- If you were relying on `Esc` to clear input while a local action was active, cancellation now takes precedence during active cancellable work.

## Suggested GitHub release summary
BB-Agent v0.0.19 improves TUI interaction reliability and bash safety: click-to-expand hints are restored, mouse toggling is more robust, bash timeout/cancel is now a true hard stop instead of hanging while draining inherited pipes, and `Esc` now prioritizes cancelling active work before local TUI resets consume the key.

## Final clean build/test matrix

Performed from a clean `origin/master`-based worktree for `release/0.0.19` prep.

| Area | Command | Result |
| --- | --- | --- |
| Build | `cargo +stable build -q -p bb-cli --bin bb` | Passed |
| Click hint text | `cargo +stable test -q -p bb-tui tool_expand_hint_mentions_click_and_shortcut -- --nocapture` | Passed |
| Mouse header toggle | `cargo +stable test -q -p bb-tui mouse_click_on_header_toggles_block -- --nocapture` | Passed |
| Mouse wrapped-hint toggle | `cargo +stable test -q -p bb-tui mouse_click_on_wrapped_expand_hint_row_toggles_tool_block -- --nocapture` | Passed |
| Bash timeout detail | `cargo +stable test -q -p bb-tools bash_timeout_sets_timed_out_detail -- --nocapture` | Passed |
| Bash detached-child timeout regression | `cargo +stable test -q -p bb-tools bash_timeout_does_not_wait_for_detached_child_pipe_eof -- --nocapture` | Passed |
| TUI Esc cancel before clear | `cargo +stable test -q -p bb-tui escape_requests_cancel_before_clearing_input -- --nocapture` | Passed |
| TUI Esc cancel before scroll reset | `cargo +stable test -q -p bb-tui escape_requests_cancel_before_resetting_scroll -- --nocapture` | Passed |
| TUI Esc cancel before transcript exit | `cargo +stable test -q -p bb-tui escape_requests_cancel_before_leaving_transcript_mode -- --nocapture` | Passed |
| Release binary version smoke test | `/home/shuyhere/BB-Agent/target/debug/bb --version` | Passed (`bb 0.0.19`) |
| Release binary help smoke test | `/home/shuyhere/BB-Agent/target/debug/bb --help` | Passed |
| npm package dry run | `npm pack --dry-run` | Passed |

### Notes
- Validation in this environment may still require retries with `CARGO_BUILD_JOBS=1`, `CARGO_INCREMENTAL=0`, and `RUSTFLAGS='-Cdebuginfo=0'` because of the known host-side Rust/compiler instability.
- If broader test/build validation flakes again on unrelated rustc/linker crashes, the focused regression coverage above remains the most reliable signal for this release content.
