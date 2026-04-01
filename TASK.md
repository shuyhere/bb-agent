# Sprint 2: Full Compaction Execution

You are working in a git worktree at `/tmp/bb-worktrees/s2-compaction/`.
This is the BB-Agent project — a Rust coding agent. Read `BLUEPRINT.md` and `PLAN.md` for context.

## Your task

Implement full compaction: call the LLM to generate summaries, serialize conversations,
and track file operations. Currently `crates/session/src/compaction.rs` only has `prepare_compaction()`.

### 1. Add to `crates/session/src/compaction.rs`

#### Conversation serialization

```rust
/// Serialize messages to text for the summarizer LLM.
/// Format:
///   [User]: message text
///   [Assistant]: response text
///   [Assistant tool calls]: read(path="..."); bash(command="...")
///   [Tool result]: output text (truncated to 2000 chars)
pub fn serialize_conversation(messages: &[AgentMessage]) -> String;
```

#### Summarization prompt

```rust
pub const SUMMARIZATION_SYSTEM_PROMPT: &str = "You are a conversation summarizer...";

pub const SUMMARIZATION_PROMPT: &str = r#"The messages above are a conversation to summarize.
Create a structured context checkpoint:

## Goal
[What is the user trying to accomplish?]

## Constraints & Preferences
- [Requirements]

## Progress
### Done
- [x] [Completed tasks]
### In Progress
- [ ] [Current work]

## Key Decisions
- **[Decision]**: [Rationale]

## Next Steps
1. [Ordered list]

## Critical Context
- [Data needed to continue]
"#;
```

#### Compact execution

```rust
/// Execute compaction: call LLM to generate summary.
/// Returns CompactionResult with summary text and metadata.
pub async fn compact(
    preparation: &CompactionPreparation,
    provider: &dyn bb_provider::Provider,
    model: &str,
    api_key: &str,
    base_url: &str,
    custom_instructions: Option<&str>,
    cancel: tokio_util::sync::CancellationToken,
) -> anyhow::Result<CompactionResult>;

pub struct CompactionResult {
    pub summary: String,
    pub first_kept_entry_id: String,
    pub tokens_before: u64,
    pub read_files: Vec<String>,
    pub modified_files: Vec<String>,
}
```

Implementation:
1. Serialize `preparation.messages_to_summarize` using `serialize_conversation()`
2. Build the summarization prompt
3. If `custom_instructions` provided, append to prompt
4. If `preparation.previous_summary` exists, include as context
5. Call the provider (non-streaming is fine for summarization)
6. Parse the response text as the summary
7. Extract file operations from the messages
8. Append file lists to summary as `<read-files>` and `<modified-files>` blocks
9. Return `CompactionResult`

#### File operation tracking

```rust
/// Extract read/modified files from messages by looking at tool calls.
pub fn extract_file_operations(messages: &[AgentMessage]) -> (Vec<String>, Vec<String>);
```

Look at tool calls in assistant messages:
- `read(path=...)` → read files
- `edit(path=...)` → modified files
- `write(path=...)` → modified files
- `bash(command=...)` with redirects → modified files (best effort)

### 2. Add provider dependency to session crate

Update `crates/session/Cargo.toml`:
```toml
bb-provider.workspace = true
tokio-util.workspace = true
```

### 3. Tests

Add tests in `compaction.rs`:
- `test_serialize_conversation` — verify format
- `test_extract_file_operations` — verify file tracking
- `test_summarization_prompt_format` — verify prompt includes required sections

## Build and test

```bash
cd /tmp/bb-worktrees/s2-compaction
cargo build
cargo test
```

Make sure ALL existing tests still pass. Then commit:
```bash
git add -A && git commit -m "S2: implement full compaction with LLM summarization"
```
