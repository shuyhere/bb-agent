Build a Markdown rendering component for BB-Agent TUI.

Work in `~/BB-Agent/crates/tui/src/markdown.rs`. Read AGENTS.md for project context.

## Task: Create `markdown.rs`

A component that takes markdown text and renders it as styled terminal output.

### Use `pulldown-cmark` for parsing

```rust
use pulldown_cmark::{Parser, Event, Tag, TagEnd, CodeBlockKind};
```

### Features to implement

1. **Headings** — bold + colored (e.g., bright white bold for h1, cyan for h2)
2. **Bold** (`**text**`) — `\x1b[1m`
3. **Italic** (`*text*`) — `\x1b[3m`
4. **Strikethrough** (`~~text~~`) — `\x1b[9m`
5. **Inline code** (`` `code` ``) — dim background or distinct color
6. **Code blocks** — bordered, with language label, syntax highlighted via `syntect`
7. **Block quotes** — prefixed with `│ ` in gray
8. **Lists** — `  • ` for unordered, `  1. ` for ordered, support nesting
9. **Links** — show text + URL in dim
10. **Horizontal rules** — `─` repeated to width
11. **Paragraphs** — separated by blank lines, word-wrapped to width

### Word wrapping
- Wrap text at word boundaries to fit terminal width
- Use `unicode-width` for correct width measurement
- Preserve ANSI escape codes across wraps

### Interface
```rust
pub struct MarkdownRenderer {
    text: String,
    cached_lines: Option<(u16, Vec<String>)>,  // (width, lines)
}

impl MarkdownRenderer {
    pub fn new(text: &str) -> Self;
    pub fn set_text(&mut self, text: &str);
    pub fn render(&mut self, width: u16) -> Vec<String>;
}
```

### Syntax highlighting for code blocks
```rust
use syntect::parsing::SyntaxSet;
use syntect::highlighting::{ThemeSet, Style};
use syntect::easy::HighlightLines;
```

Use `syntect` to highlight code blocks. Map the language tag from the markdown fence to a syntax definition. Fall back to plain text if unknown.

### Colors (use ANSI escape codes directly)
- Heading: `\x1b[1;97m` (bold bright white)
- Bold: `\x1b[1m`
- Italic: `\x1b[3m`
- Code inline: `\x1b[38;5;223m` (warm tone)
- Code block border: `\x1b[90m` (dark gray)
- Quote border: `\x1b[90m│\x1b[0m `
- List bullet: `\x1b[90m•\x1b[0m`
- Link URL: `\x1b[4;90m` (underline dim)
- Reset: `\x1b[0m`

### Build and test
```
cd ~/BB-Agent && cargo build
```

Write a simple test that renders a markdown string and checks the output has expected number of lines.
