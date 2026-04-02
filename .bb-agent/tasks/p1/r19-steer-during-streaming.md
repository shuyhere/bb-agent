# Task: allow typing steer messages during streaming

Worktree: `/tmp/bb-restructure/r19-steer`
Branch: `r19-steer-during-streaming`

## Goal
When the model is streaming a response, the user should be able to type in the editor. When they press Enter, the text is queued as a "steer" message that gets sent as the next user message after the current response finishes.

## Current behavior
In `runtime.rs` `run_streaming_turn_loop`, the `tokio::select!` terminal event branch silently consumes all key events except Esc/Ctrl-C during streaming. The editor does not receive input.

## What to change

### 1. Pass non-abort keys to the editor during streaming
In the `tokio::select!` terminal event branch in `run_streaming_turn_loop`:

```rust
TerminalEvent::Key(key) => {
    let is_esc = key.code == KeyCode::Esc && key.modifiers == KeyModifiers::NONE;
    let is_ctrl_c = key.code == KeyCode::Char('c') && key.modifiers == KeyModifiers::CONTROL;
    if is_esc || is_ctrl_c {
        self.abort_token.cancel();
        aborted = true;
        self.show_warning("Aborted");
    } else {
        // Forward to TUI (editor gets the input)
        self.ui.tui.handle_key(&key);
    }
}
```

### 2. Intercept Enter during streaming as steer
When Enter is pressed during streaming and the editor has text:
- Take the editor text
- Push it to `self.queues.steering_queue`
- Clear the editor
- Show the queued message in the pending messages area
- Don't send it yet — it'll be sent after the current response

Check: if the editor's slash menu or file menu is open, let Enter go to the menu instead.

### 3. Drain steer queue after streaming turn
After `run_streaming_turn_loop` returns in `dispatch_prompt`, the existing `drain_queued_messages()` call already handles this — it drains steering queue first, then follow-up queue.

### 4. Show pending messages
Update `rebuild_pending_container` to show queued steer messages:
```
  Queued: fix the imports too
  Queued: also run the tests
```

### 5. Handle the editor submit during streaming
In the `tokio::select!` terminal event handler, after forwarding to `self.ui.tui.handle_key(&key)`:
- Check if the editor submitted text (check `editor.take_submitted()` or similar)
- If so, queue it as a steer message

## Key files
- `crates/cli/src/interactive/controller/runtime.rs` — streaming select! loop
- `crates/cli/src/interactive/controller/rendering.rs` — pending messages display
- `crates/cli/src/interactive/controller/mode.rs` — QueueState

## Constraints
- Don't break Esc/Ctrl-C abort
- Don't break resize handling
- Editor should be visually active during streaming (cursor visible, typing works)
- Queued messages appear in pending area below chat

## Verification
```
cargo build -q
```

## Finish
```
git add -A && git commit -m "allow steer messages during streaming: type while model responds"
```
