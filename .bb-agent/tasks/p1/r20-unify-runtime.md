# Task: unify print-mode and interactive-mode streaming turn loops

Worktree: `/tmp/bb-restructure/r20-unify-runtime`
Branch: `r20-unify-runtime`

## Goal
`crates/cli/src/run.rs` (print mode) and `crates/cli/src/interactive/controller/runtime.rs` (interactive mode) both implement their own streaming turn loop with duplicated logic for:
- Building CompletionRequest
- Calling provider.stream()
- Collecting stream events
- Building assistant messages
- Appending entries to session DB
- Executing tool calls
- Looping for multi-turn tool use

Extract a shared `TurnRunner` in `crates/cli/src/turn_runner.rs` that both modes use.

## Shared turn runner API

```rust
pub struct TurnConfig {
    pub conn: &Connection,
    pub session_id: &str,
    pub system_prompt: &str,
    pub model: &Model,
    pub provider: &dyn Provider,
    pub api_key: &str,
    pub base_url: &str,
    pub tools: &[Box<dyn Tool>],
    pub tool_defs: &[Value],
    pub tool_ctx: &ToolContext,
    pub thinking: Option<&str>,
    pub cancel: CancellationToken,
}

pub enum TurnEvent {
    TextDelta(String),
    ThinkingDelta(String),
    ToolCallStart { id: String, name: String },
    ToolCallDelta { id: String, args: String },
    ToolExecuting { id: String, name: String },
    ToolResult { id: String, name: String, is_error: bool },
    TurnEnd,
    Done { text: String },
    Error(String),
}

pub async fn run_turn(
    config: TurnConfig,
    event_tx: mpsc::UnboundedSender<TurnEvent>,
) -> Result<()>;
```

## Changes

1. Create `crates/cli/src/turn_runner.rs` with the shared logic
2. Refactor `run.rs` to use `run_turn()` (print mode just collects text)
3. Refactor `runtime.rs` to use `run_turn()` (interactive mode forwards events)
4. Move `append_user_message`, `append_assistant_message`, `execute_tool_calls`, `get_leaf` into the shared module
5. Add cost computation from model pricing (currently only in runtime.rs)

## Constraints
- Keep both print and interactive modes working
- Don't change behavior
- Print mode still works non-interactively
- Interactive mode still supports abort, steer, auto-compact, auto-retry

## Verification
```
cargo build -q
bb -p --model anthropic/claude-haiku-4-5-20251001 "Reply with exactly: unified"
```

## Finish
```
git add -A && git commit -m "unify print and interactive streaming turn loops into shared TurnRunner"
```
