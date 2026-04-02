# Task: implement auto-retry on provider errors

Worktree: `/tmp/bb-restructure/r17-auto-retry`
Branch: `r17-auto-retry`

## Goal
When a provider returns a retryable error (rate limit, 5xx, overload, network), automatically retry with exponential backoff instead of showing the error and stopping.

## What to implement

In `crates/cli/src/interactive/controller/runtime.rs`, inside `run_streaming_turn_loop`:

After the provider stream task completes, check if the result was a retryable error. If so:
1. Show status: "Retrying in 2s... (attempt 1/5)"
2. Wait with `tokio::time::sleep` (abortable via Esc)
3. Remove the failed assistant message from session
4. Loop back to retry the same turn

### Retryable error detection
Match these patterns in error message (same as pi):
- `overloaded`, `rate limit`, `too many requests`
- `429`, `500`, `502`, `503`, `504`
- `service unavailable`, `server error`, `internal error`
- `network error`, `connection error`, `connection refused`
- `fetch failed`, `timed out`, `timeout`

NOT retryable: `401 Unauthorized`, `400 Bad Request`, context overflow

### Retry settings
- max retries: 5
- base delay: 2000ms
- backoff: exponential (2s, 4s, 8s, 16s, 32s)
- abortable via Esc during the wait

### Implementation in runtime.rs

After `let _ = stream_handle.await;` and the abort check, before processing `all_events`:
1. Check if stream_handle returned an error
2. If retryable, show retry status, sleep, and `continue` the loop
3. Track retry attempt count
4. On max retries exceeded, show the error normally
5. On success after retry, show "Retry succeeded (attempt N)"
6. Reset retry count on success

## Constraints
- Do NOT change the streaming select! loop itself
- Only retry the provider call, not tool execution
- Show clear feedback: "Rate limited, retrying in 4s (2/5)"
- Esc during retry wait should cancel and show the original error

## Verification
```
cargo build -q
```

## Finish
```
git add -A && git commit -m "implement auto-retry with exponential backoff for provider errors"
```
