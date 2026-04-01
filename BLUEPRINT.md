# BB-Agent Blueprint

> A Rust-native coding agent. Minimal tools, minimal prompt, maximal control.
>
> Design informed by [pi](https://github.com/badlogic/pi-mono)
> ([design post](https://mariozechner.at/posts/2025-11-30-pi-coding-agent/))
> and [ReMe/CoPaw](https://github.com/agentscope-ai/ReMe) context management.

---

## Table of Contents

1. [Philosophy](#1-philosophy)
2. [What BB-Agent is / is not](#2-scope)
3. [Architecture](#3-architecture)
4. [Session Model](#4-session-model)
5. [SQLite Schema](#5-sqlite-schema)
6. [Context Builder](#6-context-builder)
7. [Compaction](#7-compaction)
8. [Branch Summarization](#8-branch-summarization)
9. [Tools & Artifacts](#9-tools--artifacts)
10. [Provider Transport](#10-provider-transport)
11. [Hook System](#11-hook-system)
12. [Plugin Host](#12-plugin-host)
13. [TUI](#13-tui)
14. [System Prompt](#14-system-prompt)
15. [Crate Layout](#15-crate-layout)
16. [Technology Choices](#16-technology-choices)
17. [Disk Layout](#17-disk-layout)
18. [Migration & Compatibility](#18-migration--compatibility)
19. [Roadmap](#19-roadmap)
20. [Deferred Features](#20-deferred-features)

---

## 1. Philosophy

> "If I don't need it, it won't be built."  — Mario Zechner

1. **Minimal tools, maximal capability.** Four tools. Bash covers everything else.
2. **Minimal prompt.** Under 1000 tokens. Models already know what coding agents are.
3. **Minimal hooks.** One `context` hook handles context management. Don't build a framework.
4. **Full observability.** See every message, every tool call, every token.
5. **YOLO by default.** No permission prompts, no safety theater.
6. **Files over features.** Plans → files. Todos → files. Memory → files. The agent reads them.
7. **Build what you need.** Defer everything else.
8. **Rust for the right reasons.** Fast startup, low memory, efficient session handling at scale.

---

## 2. Scope

### BB-Agent is

- A Rust coding agent CLI with 4 tools and <1000-token system prompt
- SQLite-native session storage for large-scale session management
- A session tree with branching, compaction, and navigation
- A TypeScript plugin system with hooks that can cancel/block/modify
- Artifact offload for large tool outputs
- Multi-provider with mid-session model switching
- Fully observable — nothing hidden

### BB-Agent is NOT (v1)

- A framework for building arbitrary agents
- A platform with multiple plugin runtimes
- A context management framework with policies/middleware
- A memory system with vector search
- A daemon with multi-client support
- MCP, plan mode, sub-agent orchestration, or background bash

---

## 3. Architecture

```text
┌──────────────────────────────────┐
│          User Interface          │
│     TUI (ratatui + crossterm)    │
│     Print mode / RPC mode        │
└──────────────────────────────────┘
               │
┌──────────────────────────────────┐
│          Agent Loop              │
│  prompt → LLM → tools → repeat  │
│  message queuing, abort support  │
└──────────────────────────────────┘
               │
┌──────────────────────────────────┐
│          Core Engine             │
│                                  │
│  Session    │ Context  │ Tools   │
│  (SQLite)   │ Builder  │ (4)     │
│             │          │         │
│  Compaction │ Provider │ Hooks   │
│             │ (HTTP)   │         │
│             │          │         │
│  Artifacts  │          │         │
└──────────────────────────────────┘
               │
┌──────────────────────────────────┐
│        Plugin Host               │
│  Node child process (JSON-RPC)   │
│  TypeScript plugins              │
└──────────────────────────────────┘
               │
┌──────────────────────────────────┐
│        Storage                   │
│  SQLite (sessions + indexes)     │
│  Filesystem (artifacts)          │
└──────────────────────────────────┘
```

---

## 4. Session Model

### 4.1 Core concepts

A session is an append-only sequence of entries forming a tree via
`id` / `parentId`. One branch tip is active: the **leaf**. Context for
the LLM is rebuilt from the root → leaf path. Entries are never mutated
or deleted.

### 4.2 Entry ID

```rust
/// 8-character hex identifier, unique within a session.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EntryId(pub String);
```

### 4.3 Entry types

```rust
pub struct EntryBase {
    pub id: EntryId,
    pub parent_id: Option<EntryId>,
    pub timestamp: DateTime<Utc>,
}

pub enum SessionEntry {
    Message(MessageEntry),
    Compaction(CompactionEntry),
    BranchSummary(BranchSummaryEntry),
    ModelChange(ModelChangeEntry),
    ThinkingLevelChange(ThinkingLevelEntry),
    Custom(CustomEntry),           // plugin state, not in LLM context
    CustomMessage(CustomMessageEntry), // plugin message, in LLM context
    SessionInfo(SessionInfoEntry), // display name
    Label(LabelEntry),             // bookmark on an entry
}
```

### 4.4 Message types

```rust
pub enum AgentMessage {
    User(UserMessage),
    Assistant(AssistantMessage),
    ToolResult(ToolResultMessage),
    BashExecution(BashExecutionMessage),
    Custom(CustomMessage),
    BranchSummary(BranchSummaryMessage),
    CompactionSummary(CompactionSummaryMessage),
}

pub struct UserMessage {
    pub content: Vec<ContentBlock>,
    pub timestamp: i64,
}

pub struct AssistantMessage {
    pub content: Vec<AssistantContent>,
    pub provider: String,
    pub model: String,
    pub usage: Usage,
    pub stop_reason: StopReason,
    pub error_message: Option<String>,
    pub timestamp: i64,
}

pub struct ToolResultMessage {
    pub tool_call_id: String,
    pub tool_name: String,
    pub content: Vec<ContentBlock>,
    pub details: Option<serde_json::Value>,
    pub is_error: bool,
    pub timestamp: i64,
}

pub enum ContentBlock {
    Text { text: String },
    Image { data: String, mime_type: String },
}

pub enum AssistantContent {
    Text { text: String },
    Thinking { thinking: String },
    ToolCall { id: String, name: String, arguments: serde_json::Value },
}

pub struct Usage {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub total_tokens: u64,
    pub cost: Cost,
}
```

### 4.5 Compaction entry

```rust
pub struct CompactionEntry {
    pub base: EntryBase,
    pub summary: String,
    pub first_kept_entry_id: EntryId,
    pub tokens_before: u64,
    pub details: Option<serde_json::Value>, // { readFiles, modifiedFiles }
    pub from_plugin: bool,
}
```

### 4.6 Branch summary entry

```rust
pub struct BranchSummaryEntry {
    pub base: EntryBase,
    pub from_id: EntryId,
    pub summary: String,
    pub details: Option<serde_json::Value>,
    pub from_plugin: bool,
}
```

### 4.7 Session header

```rust
pub struct SessionHeader {
    pub version: u32,          // current: 1
    pub session_id: Uuid,
    pub timestamp: DateTime<Utc>,
    pub cwd: PathBuf,
    pub parent_session: Option<String>,
}
```

### 4.8 parentId assignment

Automatic. When appending a new entry:
- `parent_id = current leaf_id`
- then `leaf_id = new entry's id`

Branching: move the leaf to an earlier entry, then the next append
creates a child there.

---

## 5. SQLite Schema

Three tables. Add more only when profiling proves a need.

```sql
-- Canonical append-only session event log
CREATE TABLE entries (
    session_id TEXT    NOT NULL,
    seq        INTEGER NOT NULL,
    entry_id   TEXT    NOT NULL,
    parent_id  TEXT,
    type       TEXT    NOT NULL,
    timestamp  TEXT    NOT NULL,
    payload    TEXT    NOT NULL,   -- full JSON
    PRIMARY KEY (session_id, seq)
);
CREATE UNIQUE INDEX idx_entry_id ON entries(session_id, entry_id);
CREATE INDEX idx_entry_parent ON entries(session_id, parent_id);

-- Session metadata
CREATE TABLE sessions (
    session_id  TEXT PRIMARY KEY,
    cwd         TEXT    NOT NULL,
    created_at  TEXT    NOT NULL,
    updated_at  TEXT    NOT NULL,
    name        TEXT,
    leaf_id     TEXT,
    entry_count INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX idx_sessions_cwd ON sessions(cwd);

-- Schema versioning
CREATE TABLE schema_version (
    version    INTEGER PRIMARY KEY,
    applied_at TEXT NOT NULL
);
```

**Why 3 tables is enough:**
- Children: query `WHERE parent_id = ?` on `entries`. Indexed.
- Labels: stored as `type = 'label'` entries. Query by type.
- Token estimates: computed on demand, cached in-memory per session.
- Session state (leaf, model, thinking): maintained in `sessions` table + derived from entries.
- Artifacts: filesystem + simple directory listing. No metadata table needed until proven otherwise.

---

## 6. Context Builder

One function. Walk root → leaf, apply compaction boundary, return messages.

```rust
pub fn build_context(
    db: &Connection,
    session_id: &str,
    leaf_id: Option<&str>,
) -> Result<SessionContext> {
    let path = walk_root_to_leaf(db, session_id, leaf_id)?;

    let mut messages = Vec::new();
    let mut model = None;
    let mut thinking_level = ThinkingLevel::Off;

    // Find last compaction on path
    let compaction = path.iter().rev()
        .find(|e| e.entry_type == "compaction");

    if let Some(comp) = compaction {
        let comp_idx = path.iter().position(|e| e.id == comp.id).unwrap();
        let first_kept = &comp.first_kept_entry_id;

        // 1. Emit compaction summary
        messages.push(comp.to_summary_message());

        // 2. Emit kept messages before compaction
        let mut found = false;
        for entry in &path[..comp_idx] {
            if &entry.id == first_kept { found = true; }
            if found { try_append(&mut messages, entry); }
        }

        // 3. Emit messages after compaction
        for entry in &path[comp_idx + 1..] {
            try_append(&mut messages, entry);
            update_settings(entry, &mut model, &mut thinking_level);
        }
    } else {
        for entry in &path {
            try_append(&mut messages, entry);
            update_settings(entry, &mut model, &mut thinking_level);
        }
    }

    Ok(SessionContext { messages, model, thinking_level })
}
```

After this, the `context` hook fires once, giving plugins the full
message list to filter, inject, or trim. That single hook replaces the
6-stage context pipeline from earlier plans.

---

## 7. Compaction

### 7.1 Trigger

```rust
fn should_compact(context_tokens: u64, context_window: u64, reserve: u64) -> bool {
    context_tokens > context_window - reserve
}
```

Defaults: `reserve_tokens = 16384`, `keep_recent_tokens = 20000`.

### 7.2 Algorithm

1. Find previous compaction on active path (if any).
2. Boundary starts at previous compaction's `first_kept_entry_id`.
3. Walk backward from newest, accumulate estimated tokens.
4. When accumulated ≥ `keep_recent_tokens`, cut at nearest valid point.
5. Valid cut points: user messages, assistant messages, bash executions, custom messages. Never tool results alone.
6. If one turn exceeds `keep_recent_tokens`, split it (turn prefix + suffix).
7. Summarize with structured prompt. Track file operations.

### 7.3 Split-turn

When a single turn is too large:
- summarize its prefix
- keep its suffix
- merge both summaries

### 7.4 Iterative compaction

On repeated compaction, the summarization span starts at the previous
compaction's `first_kept_entry_id`, so previously-kept messages get
included in the next pass.

### 7.5 Summary format

```markdown
## Goal
[What the user is trying to accomplish]

## Constraints & Preferences
- [Requirements]

## Progress
### Done
- [x] [Completed]
### In Progress
- [ ] [Current]

## Key Decisions
- **[Decision]**: [Rationale]

## Next Steps
1. [What should happen next]

<read-files>
path/to/file.rs
</read-files>

<modified-files>
path/to/changed.rs
</modified-files>
```

### 7.6 Plugin override

A plugin can replace the entire compaction via `session_before_compact`:

```typescript
bb.on("session_before_compact", async (event, ctx) => {
    return {
        compaction: {
            summary: myCustomSummary,
            firstKeptEntryId: event.preparation.firstKeptEntryId,
            tokensBefore: event.preparation.tokensBefore,
        }
    };
});
```

No `CompactionStrategy` trait registry needed.

---

## 8. Branch Summarization

### When

User navigates to a different branch via `/tree` and chooses to summarize.

### What gets summarized

Entries from old leaf back to common ancestor with target:

```text
         ┌─ C ─ D (old leaf)
    A ── B
         └─ E ─ F (target)

Common ancestor: B
Summarize: C, D
```

### Budget

Walk entries newest → oldest under token budget. Keeps most recent
branch context first. File operations accumulated cumulatively.

### Storage

Branch summary entry appended at the new branch position.

---

## 9. Tools & Artifacts

### 9.1 Four tools

| Tool | What it does |
|------|-------------|
| `read` | Read file contents (text + images). Offset/limit for large files. Output capped at 2000 lines / 50KB. |
| `bash` | Execute shell command. Streaming stdout/stderr. Optional timeout. Cancellable. Output capped at 2000 lines / 50KB. |
| `edit` | Exact text replacement. `oldText` must match exactly. |
| `write` | Create or overwrite file. Auto-creates parent directories. |

Additional read-only tools (`grep`, `find`, `ls`) can be enabled via
`--tools read,bash,grep,find,ls` for restricted mode.

### 9.2 Artifact offload

When a tool result exceeds a size threshold:

```rust
const ARTIFACT_THRESHOLD: usize = 100 * 1024; // 100KB
const AGED_THRESHOLD: usize = 3 * 1024;       // 3KB

fn maybe_offload(content: &str, artifacts_dir: &Path) -> (String, Option<PathBuf>) {
    if content.len() <= ARTIFACT_THRESHOLD {
        return (content.to_string(), None);
    }
    let path = artifacts_dir.join(format!("{}.txt", Uuid::new_v4()));
    fs::write(&path, content).ok();
    let truncated = &content[..ARTIFACT_THRESHOLD];
    let hint = format!(
        "{truncated}\n\n[Truncated. Full output ({} bytes) saved to {}. \
         Use read tool to access.]",
        content.len(), path.display()
    );
    (hint, Some(path))
}
```

When messages age beyond a recent window (configurable, default 3 turns),
tool results can be further truncated to `AGED_THRESHOLD`. This happens
inside the `context` hook or as a pre-processing step before context
building. No separate pipeline stage needed.

### 9.3 File mutation queue

Tools that write files participate in a per-file async queue so parallel
tool calls don't clobber each other.

---

## 10. Provider Transport

### 10.1 Trait

```rust
pub trait Provider: Send + Sync {
    fn name(&self) -> &str;

    async fn stream(
        &self,
        request: CompletionRequest,
        options: RequestOptions,
    ) -> Result<Pin<Box<dyn Stream<Item = StreamEvent> + Send>>>;
}
```

### 10.2 Built-in providers

| Provider | API |
|----------|-----|
| Anthropic | Messages API |
| OpenAI | Completions API + Responses API |
| Google | Generative AI API |
| Custom | Any OpenAI-compatible endpoint |

### 10.3 Context handoff

When switching providers mid-session:
- thinking traces converted to `<thinking>` tagged text blocks
- provider-specific signed blobs stripped
- tool call formats normalized

### 10.4 Model registry

```rust
pub struct Model {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub api: ApiType,            // anthropic, openai-completions, openai-responses, google
    pub context_window: u64,
    pub max_tokens: u64,
    pub reasoning: bool,
    pub input: Vec<InputType>,   // text, image
    pub cost: CostConfig,
    pub base_url: Option<String>,
}
```

Models can be configured via settings JSON for self-hosted or custom providers.

### 10.5 Abort support

Every provider call accepts a `CancellationToken`. Partial results are
returned on abort, not discarded.

---

## 11. Hook System

### 11.1 Events (v1)

14 events. Add more when plugins need finer granularity.

```text
Session:
  session_start
  session_shutdown
  session_before_compact       can cancel or override summary
  session_compact              notification
  session_before_tree          can cancel or override summary
  session_tree                 notification

Agent:
  before_agent_start           can inject message, modify system prompt
  agent_end                    notification

Turn:
  turn_start                   notification
  turn_end                     notification

Tool:
  tool_call                    can block, mutate args
  tool_result                  can modify content

Context:
  context                      can filter/modify full message list

Provider:
  before_provider_request      can inspect/replace payload
```

### 11.2 How `context` hook replaces a pipeline

A plugin can do everything a multi-stage pipeline would, in one hook:

```typescript
bb.on("context", async (event, ctx) => {
    let messages = event.messages;

    // Age old tool results
    messages = messages.map(msg => {
        if (msg.role === "toolResult" && isOld(msg) && isBig(msg)) {
            return truncate(msg, 2000);
        }
        return msg;
    });

    // Inject RAG context
    const usage = ctx.getContextUsage();
    if (usage && usage.tokens < usage.contextWindow * 0.8) {
        const chunks = await vectorDB.search(lastUserMessage(messages));
        if (chunks.length) {
            messages.push(makeUserMessage(formatChunks(chunks)));
        }
    }

    // Trim if over budget
    while (estimateTokens(messages) > usage.contextWindow - 16384) {
        const oldest = findOldestLowPriority(messages);
        if (!oldest) break;
        messages.splice(messages.indexOf(oldest), 1);
    }

    return { messages };
});
```

### 11.3 Hook return semantics

| Event | Return | Effect |
|-------|--------|--------|
| `tool_call` | `{ block, reason }` | Block execution |
| `tool_result` | `{ content, details, isError }` | Replace result |
| `context` | `{ messages }` | Replace message list |
| `session_before_compact` | `{ cancel, compaction }` | Cancel or override |
| `session_before_tree` | `{ cancel, summary }` | Cancel or override |
| `before_agent_start` | `{ message, systemPrompt }` | Inject context, modify prompt |
| `before_provider_request` | raw payload | Replace provider payload |
| `input` | `{ action, text }` | Intercept/transform input |

### 11.4 Event bus

```rust
pub struct EventBus {
    handlers: HashMap<&'static str, Vec<HandlerEntry>>,
}

struct HandlerEntry {
    plugin_id: String,
    handler: Box<dyn EventHandler>,
}

pub trait EventHandler: Send + Sync {
    async fn handle(&self, event: &Event, ctx: &HookContext)
        -> Result<Option<HookResult>>;
}
```

---

## 12. Plugin Host

TypeScript only. One runtime. Via JSON-RPC over stdio to a Node child process.

```text
BB-Agent (Rust) ←── JSON-RPC (stdio) ──→ Node child process
                                              │
                                         host.js
                                              │
                                    ┌─────────┼─────────┐
                                    │         │         │
                               plugin1.ts plugin2.ts  ...
```

### 12.1 Plugin API

```typescript
import type { PluginAPI } from "@bb-agent/plugin-api";

export default function (bb: PluginAPI) {
    // Events
    bb.on("tool_call", async (event, ctx) => {
        if (event.toolName === "bash" && event.input.command.includes("rm -rf")) {
            const ok = await ctx.ui.confirm("Dangerous!", "Allow rm -rf?");
            if (!ok) return { block: true, reason: "Blocked by user" };
        }
    });

    // Custom tool
    bb.registerTool({
        name: "greet",
        description: "Greet someone",
        parameters: { type: "object", properties: { name: { type: "string" } } },
        async execute(toolCallId, params) {
            return { content: [{ type: "text", text: `Hello, ${params.name}!` }] };
        },
    });

    // Command
    bb.registerCommand("review", {
        description: "Run code review",
        handler: async (args, ctx) => { /* ... */ },
    });

    // Persist state
    bb.appendEntry("my-plugin", { key: "value" });
}
```

### 12.2 Context available to plugins

```typescript
ctx.sessionManager.getEntries()
ctx.sessionManager.getBranch()
ctx.sessionManager.getLeafId()
ctx.getContextUsage()        // { tokens, contextWindow, percent }
ctx.model                    // current model info
ctx.cwd                      // working directory
ctx.signal                   // abort signal
ctx.ui.notify(msg, level)
ctx.ui.confirm(title, msg)
ctx.ui.select(title, options)
ctx.ui.input(title, placeholder)
ctx.ui.setStatus(key, text)
```

### 12.3 Plugin discovery

```text
~/.bb-agent/plugins/*.ts            global
~/.bb-agent/plugins/*/index.ts      global (directory)
<project>/.bb-agent/plugins/*.ts    project-local
```

Lua and WASI plugin hosts are deferred until someone needs them.

---

## 13. TUI

Scrollback-based, not full-screen. Same approach as pi.

- `ratatui` + `crossterm`
- Differential rendering with synchronized output (`CSI ?2026h/l`)
- Components: chat view, editor with autocomplete, tree selector, status bar
- Theme support via JSON files
- Modes: interactive, print (`--print`), RPC

No custom footer/header/widget factories. Add when needed.

---

## 14. System Prompt

Under 1000 tokens. Appended with project `AGENTS.md`.

```text
You are an expert coding assistant. You help users by reading files,
executing commands, editing code, and writing new files.

Available tools:
- read: Read file contents (text and images), with offset/limit for large files
- bash: Execute bash commands with optional timeout
- edit: Make precise edits with exact text replacement
- write: Create or overwrite files

Guidelines:
- Use bash for file operations like ls, grep, find
- Use read to examine files before editing
- Use edit for precise changes (old text must match exactly)
- Use write only for new files or complete rewrites
- Be concise in your responses
- Show file paths clearly when working with files
```

Project context: `AGENTS.md` loaded hierarchically (global → project).
Full system prompt replacement supported via `AGENTS.md` directive.

---

## 15. Crate Layout

```text
bb-agent/
  Cargo.toml                # workspace
  crates/
    core/                   # types, config, error, agent loop
      src/
        types.rs            # EntryId, AgentMessage, ContentBlock, ...
        config.rs           # settings loading
        error.rs            # BbError
        agent.rs            # agent loop: prompt → LLM → tools → repeat
        lib.rs

    session/                # SQLite store, tree, context builder, compaction
      src/
        store.rs            # append, load, query entries
        schema.rs           # CREATE TABLE, migrations
        tree.rs             # get_tree, get_branch, common_ancestor
        context.rs          # build_context (root → leaf → messages)
        compaction.rs       # prepare, cut point, compact, branch summary
        import_export.rs    # JSONL ↔ SQLite
        lib.rs

    tools/                  # read, bash, edit, write + artifact offload
      src/
        read.rs
        bash.rs
        edit.rs
        write.rs
        artifacts.rs        # maybe_offload, truncation, cleanup
        scheduler.rs        # parallel execution, file mutation queue
        lib.rs

    provider/               # model registry, HTTP streaming, multi-provider
      src/
        registry.rs         # Model, ModelRegistry
        anthropic.rs
        openai.rs
        google.rs
        custom.rs           # any OpenAI-compatible endpoint
        handoff.rs          # cross-provider context conversion
        lib.rs

    hooks/                  # event bus + hook dispatch
      src/
        bus.rs              # EventBus, handler registry, dispatch
        events.rs           # all event type definitions
        results.rs          # HookResult, merge logic
        lib.rs

    plugin-host/            # Node/TS plugin bridge
      src/
        host.rs             # spawn Node, manage lifecycle
        bridge.rs           # JSON-RPC ↔ Rust event translation
        discovery.rs        # find plugins on disk
        lib.rs

    tui/                    # terminal UI
      src/
        app.rs              # main loop, differential rendering
        chat.rs             # message display, streaming
        editor.rs           # input, autocomplete, file search
        tree.rs             # /tree selector
        status.rs           # footer, model/token info
        theme.rs            # JSON theme loading
        lib.rs

    cli/                    # binary entrypoint
      src/
        main.rs             # arg parsing, mode dispatch
```

**8 crates.** Each has a clear, single responsibility.

---

## 16. Technology Choices

| Need | Choice | Why |
|------|--------|-----|
| Async | `tokio` | Standard |
| Serialization | `serde` + `serde_json` | Standard |
| SQLite | `rusqlite` | Mature, `spawn_blocking` for async |
| TUI | `ratatui` + `crossterm` | Best Rust TUI ecosystem |
| HTTP | `reqwest` | Streaming + async |
| CLI | `clap` | Standard |
| UUID | `uuid` | Standard |
| Time | `chrono` | Standard |
| Logging | `tracing` | Structured, async-aware |
| Diff | `similar` | For edit verification |
| Process | `tokio::process` | Async subprocess |

---

## 17. Disk Layout

```text
~/.bb-agent/
  sessions.db             # all sessions (SQLite)
  settings.json           # global settings
  plugins/                # global plugins (*.ts)
  artifacts/              # tool output offload ({uuid}.txt)
  AGENTS.md               # global project context

<project>/
  .bb-agent/
    settings.json         # project settings
    plugins/              # project plugins
  AGENTS.md               # project context (can also be at root)
```

---

## 18. Migration & Compatibility

### JSONL import

```rust
pub fn import_pi_session(jsonl_path: &Path, db: &Connection) -> Result<Uuid>
```

- Reads pi v1/v2/v3 session JSONL
- Inserts into SQLite preserving all entry ids and parent links
- Populates `sessions` table

### JSONL export

```rust
pub fn export_session_jsonl(db: &Connection, session_id: &Uuid, out: &Path) -> Result<()>
```

- Writes pi-compatible JSONL for backup, debug, sharing

### Bulk migration

```bash
bb-agent migrate --from-pi ~/.pi/agent/sessions/
```

---

## 19. Roadmap

### Phase 1: Core engine (Weeks 1–5)

| Week | Deliverable | Crate |
|------|-------------|-------|
| 1 | Types, config, error, settings | `core` |
| 2 | SQLite store, append, load, tree queries | `session` |
| 3 | Context builder, compaction (prepare + execute) | `session` |
| 4 | Tools: read, bash, edit, write + artifact offload | `tools` |
| 5 | Agent loop: prompt → stream → tools → repeat | `core` |

**Gate**: run a coding session with one provider, print mode, no TUI.

### Phase 2: Providers (Weeks 6–7)

| Week | Deliverable | Crate |
|------|-------------|-------|
| 6 | Anthropic + OpenAI streaming | `provider` |
| 7 | Google + custom + context handoff | `provider` |

**Gate**: multi-provider with mid-session switching.

### Phase 3: TUI (Weeks 8–10)

| Week | Deliverable | Crate |
|------|-------------|-------|
| 8 | Terminal rendering, chat view, streaming display | `tui` |
| 9 | Editor, slash commands, session selector (/resume) | `tui` |
| 10 | Tree selector (/tree), model selector, themes | `tui` |

**Gate**: interactive coding agent in terminal.

### Phase 4: Hooks + Plugins (Weeks 11–13)

| Week | Deliverable | Crate |
|------|-------------|-------|
| 11 | Event bus, hook dispatch, result merging | `hooks` |
| 12 | Node/TS plugin host, JSON-RPC bridge | `plugin-host` |
| 13 | Plugin discovery, example plugins | `plugin-host` |

**Gate**: TS plugins can register tools and intercept events.

### Phase 5: Polish (Weeks 14–16)

| Week | Deliverable | Crate |
|------|-------------|-------|
| 14 | JSONL import/export, pi migration | `session` |
| 15 | Print mode, RPC mode | `cli` |
| 16 | Docs, packaging, release | all |

**Gate**: ready for use.

---

## 20. Deferred Features

Explicitly not in v1. Build only when a real use case demands it.

| Feature | Trigger to build |
|---------|-----------------|
| Lua plugin host | Someone needs lightweight embedded scripting |
| WASI plugin host | Someone needs sandboxed compiled plugins |
| Daemon mode | IDE integration demands persistent background process |
| Long-term memory system | File-based memory proves insufficient |
| FTS over sessions | Large-scale session search is needed |
| `entry_children` table | Tree queries are measurably slow |
| `token_cache` table | Token estimation is measurably slow |
| `artifacts` metadata table | Artifact management needs DB queries |
| Additional hook events | Plugins need finer event granularity |
| Context pipeline stages | Single `context` hook proves insufficient |
| Fork support | Branch extraction to separate session files needed |
| Multiple edit operations per call | Edit tool needs batching |
| Background bash | tmux not sufficient for use case |
| Sub-agent orchestration | bash self-spawn not sufficient |
| MCP | CLI tools + READMEs not sufficient |
