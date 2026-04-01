# W4: Streaming bash display + edit diff

Working dir: `/tmp/bb-w/w4-streaming-bash-diff/`

## Problem
1. Bash tool waits for process to finish before showing any output — no real-time feedback
2. Edit tool shows "Applied N/N edit(s)" with no diff — user can't see what changed

## Tasks

### 1. Add streaming callback to bash tool

Modify `crates/tools/src/bash.rs`:

Add an `on_output` callback that streams chunks as they arrive:

```rust
pub struct BashTool;

// Add a way to set a streaming callback
// The simplest approach: add it to ToolContext
```

Actually, modify `ToolContext` in `crates/tools/src/lib.rs`:
```rust
pub struct ToolContext {
    pub cwd: PathBuf,
    pub artifacts_dir: PathBuf,
    pub on_output: Option<Box<dyn Fn(&str) + Send + Sync>>,
}
```

Then in `bash.rs`, stream chunks as they arrive:
```rust
// Instead of reading all stdout at once, read in chunks:
let mut stdout = child.stdout.take().unwrap();
let mut buf = [0u8; 4096];
loop {
    let n = stdout.read(&mut buf).await?;
    if n == 0 { break; }
    let chunk = String::from_utf8_lossy(&buf[..n]);
    if let Some(ref on_output) = ctx.on_output {
        on_output(&chunk);
    }
    stdout_buf.extend_from_slice(&buf[..n]);
}
```

This way the TUI can display bash output in real-time as it streams.

### 2. Create `crates/tools/src/diff.rs` — edit diff display

When the edit tool modifies a file, show a colored inline diff.

```rust
use similar::{ChangeTag, TextDiff};

pub struct DiffLine {
    pub tag: ChangeTag,  // Equal, Delete, Insert
    pub content: String,
}

/// Generate a unified diff between old and new text.
pub fn generate_diff(old: &str, new: &str, context_lines: usize) -> Vec<DiffLine> {
    let diff = TextDiff::from_lines(old, new);
    let mut lines = Vec::new();

    for change in diff.iter_all_changes() {
        lines.push(DiffLine {
            tag: change.tag(),
            content: change.value().to_string(),
        });
    }

    lines
}

/// Render diff lines with ANSI colors.
pub fn render_diff(diff_lines: &[DiffLine]) -> Vec<String> {
    diff_lines.iter().map(|line| {
        let prefix = match line.tag {
            ChangeTag::Equal => "  ",
            ChangeTag::Delete => "- ",
            ChangeTag::Insert => "+ ",
        };
        let color = match line.tag {
            ChangeTag::Equal => "",
            ChangeTag::Delete => "\x1b[31m",  // red
            ChangeTag::Insert => "\x1b[32m",  // green
        };
        let reset = if color.is_empty() { "" } else { "\x1b[0m" };
        format!("    {}{}{}{}", color, prefix, line.content.trim_end(), reset)
    }).collect()
}
```

### 3. Integrate diff into edit tool

Modify `crates/tools/src/edit.rs`:

Before applying the edit, save the old content. After applying, generate and display the diff:

```rust
// In execute():
let old_content = tokio::fs::read_to_string(&path).await?;

// ... apply edits ...

let new_content = tokio::fs::read_to_string(&path).await?;

// Generate diff
let diff_lines = diff::generate_diff(&old_content, &new_content, 3);
let rendered = diff::render_diff(&diff_lines);

// Include diff in result
let mut msg = format!("Applied {applied}/{} edit(s) to {path_str}", edits.len());
if !rendered.is_empty() {
    msg.push('\n');
    msg.push_str(&rendered.join("\n"));
}
```

### 4. Update lib.rs

Add `pub mod diff;` to `crates/tools/src/lib.rs`.

### 5. Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_diff() {
        let old = "line1\nline2\nline3\n";
        let new = "line1\nmodified\nline3\n";
        let diff = generate_diff(old, new, 1);
        assert!(diff.iter().any(|l| l.tag == ChangeTag::Delete));
        assert!(diff.iter().any(|l| l.tag == ChangeTag::Insert));
    }

    #[test]
    fn test_render_diff() {
        let old = "hello\n";
        let new = "world\n";
        let diff = generate_diff(old, new, 0);
        let rendered = render_diff(&diff);
        assert!(rendered.iter().any(|l| l.contains("hello")));
        assert!(rendered.iter().any(|l| l.contains("world")));
    }
}
```

### Build and test
```bash
cd /tmp/bb-w/w4-streaming-bash-diff
cargo build && cargo test
git add -A && git commit -m "W4: streaming bash + edit diff display"
```
