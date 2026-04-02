# Task: move file IO out of core crate

Worktree: `/tmp/bb-restructure/r12-core-io`
Branch: `r12-move-io-out-of-core`

## Goal
3 files in `crates/core/src/` contain direct file IO (`std::fs`), violating principle 7 (push side effects to edges):

1. `crates/core/src/config.rs` — reads config files from disk
2. `crates/core/src/settings.rs` — reads/writes settings files
3. `crates/core/src/agent_session/agents_md.rs` — reads AGENTS.md from disk

Core should contain pure types and logic. IO should live at the edge (cli crate or a dedicated IO layer).

## Method
For each file:

1. **Separate the pure logic from the IO**:
   - Keep types, parsing, validation in core
   - Extract the `std::fs::read_to_string`, `std::fs::write`, path resolution into separate functions

2. **Make the IO functions take data as input instead of reading from disk**:
   - e.g. `load_agents_md(cwd: &Path) -> Option<String>` reads from disk
   - change to: keep a `parse_agents_md(content: &str) -> ...` in core, move the file-reading wrapper to where it's called from

3. **If moving the IO wrapper to cli is too invasive**, an acceptable intermediate step is:
   - Mark the IO functions with a comment `// IO boundary — should migrate to cli`
   - Extract a `from_str` / `parse` pure version alongside the IO version
   - This keeps the refactor safe while establishing the pattern

## Constraints
- Do NOT break any functionality.
- Preserve all public API shapes.
- Minimal changes to call sites.
- If full extraction is too risky, do the intermediate step (add pure parsing functions, mark IO boundaries).

## Verification
```
cargo build -q
cargo test -q -p bb-core
```

## Finish
```
git add -A && git commit -m "separate IO from pure logic in core crate"
```
