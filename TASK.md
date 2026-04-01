# Sprint 1: Agent Session + Agent Loop

You are working in a git worktree at `/tmp/bb-worktrees/s1-agent-session/`.
This is the BB-Agent project — a Rust coding agent. Read `BLUEPRINT.md` and `PLAN.md` for context.

## Your task

Extract the agent loop from `crates/cli/src/run.rs` into a proper `AgentSession` and `AgentLoop`.

### 1. Create `crates/core/src/session.rs`

The `AgentSession` manages the full lifecycle of a coding session.

```rust
use crate::types::*;
use bb_session::store;
use bb_provider::Provider;

pub struct AgentSession {
    pub conn: rusqlite::Connection,
    pub session_id: String,
    pub system_prompt: String,
    pub model: bb_provider::registry::Model,
    pub provider: Box<dyn Provider>,
    pub api_key: String,
    pub base_url: String,
    pub tools: Vec<Box<dyn bb_tools::Tool>>,
    pub tool_defs: Vec<serde_json::Value>,
    pub tool_ctx: bb_tools::ToolContext,
    pub compaction_settings: CompactionSettings,
}

impl AgentSession {
    /// Run a single user prompt through the full agent loop.
    /// Returns a stream of AgentLoopEvents via a channel.
    pub async fn run_prompt(&self, prompt: &str, tx: tokio::sync::mpsc::UnboundedSender<AgentLoopEvent>) -> Result<()>;

    /// Get current context usage.
    pub fn context_usage(&self) -> Option<ContextUsage>;

    /// Trigger manual compaction.
    pub async fn compact(&self, instructions: Option<&str>) -> Result<()>;

    /// Check if auto-compaction should trigger, and run it if so.
    pub async fn maybe_auto_compact(&self) -> Result<bool>;
}
```

### 2. Create `crates/core/src/agent_loop.rs`

The inner turn loop:

```rust
pub enum AgentLoopEvent {
    TurnStart { turn_index: u32 },
    TextDelta { text: String },
    ThinkingDelta { text: String },
    ToolCallStart { id: String, name: String },
    ToolCallDelta { id: String, args_delta: String },
    ToolExecuting { id: String, name: String },
    ToolResult { id: String, name: String, content: String, is_error: bool },
    TurnEnd { turn_index: u32 },
    AssistantDone,
    Error { message: String },
}
```

The loop:
1. Build context from session (using `bb_session::context::build_context`)
2. Convert messages to provider format
3. Call provider with streaming (using `provider.stream()`)
4. Forward streaming events to the channel as `AgentLoopEvent`s
5. Collect final response, build `AssistantMessage`, append to session
6. If tool calls present:
   a. Execute each tool
   b. Send `ToolExecuting` and `ToolResult` events
   c. Append `ToolResultMessage` entries to session
   d. Loop back to step 1
7. If no tool calls: send `AssistantDone`, exit loop
8. After each turn: check auto-compaction

### 3. Modify `crates/cli/src/run.rs`

Refactor to use `AgentSession`:
- Remove the inline agent loop code
- Create `AgentSession` with all config
- Call `session.run_prompt(text, tx)`
- Receive events from channel
- Display events (keep current inline display for now)

### 4. Update `crates/core/src/lib.rs`

Add new modules:
```rust
pub mod session;
pub mod agent_loop;
```

### 5. Update `crates/core/Cargo.toml`

Add dependencies needed:
```toml
bb-session.workspace = true
bb-tools.workspace = true
bb-provider.workspace = true
tokio-util.workspace = true
```

## Build and test

```bash
cd /tmp/bb-worktrees/s1-agent-session
cargo build
cargo test
```

Make sure ALL existing tests still pass. Then commit:
```bash
git add -A && git commit -m "S1: extract AgentSession and AgentLoop from run.rs"
```
