Rebuild the Editor component for BB-Agent TUI to be a proper multi-line terminal editor.

Work in `~/BB-Agent/crates/tui/src/editor.rs`. Read AGENTS.md for project context.

## Task: Rebuild `editor.rs`

The editor is the user input component at the bottom of the terminal. It must support:

### Core editing
- Multi-line text buffer (Vec<Vec<char>> — one Vec<char> per line)
- Cursor position (row, col) within the buffer
- Insert characters at cursor
- Backspace / Delete
- Enter to add newline OR submit (submit when on single line, newline when multi-line with Shift+Enter or vice versa — use Enter=submit, Alt+Enter=newline)

### Cursor movement
- Left/Right arrows (character by character)
- Up/Down arrows (move between lines, or browse history if single-line)
- Home/End (start/end of line)
- Ctrl+A / Ctrl+E (start/end of line — Emacs bindings)
- Ctrl+Left / Ctrl+Right or Alt+Left / Alt+Right (word jump)

### Editing operations
- Ctrl+K — kill to end of line
- Ctrl+U — clear entire line
- Ctrl+W — delete word backward

### History
- Store previously submitted inputs
- Up arrow on first line → browse history backward
- Down arrow on last line → browse history forward

### Rendering
- Use crossterm for keyboard events
- Render the editor with a configurable prompt string (e.g., "> ")
- Show cursor position visually
- Word-wrap long lines to terminal width
- Return `Vec<String>` of rendered lines from `render(width)`

### Interface
```rust
pub struct Editor {
    // ...
}

impl Editor {
    pub fn new(prompt: &str) -> Self;
    pub fn read_line(&mut self) -> Option<String>;  // blocking read
    pub fn get_text(&self) -> String;
    pub fn set_text(&mut self, text: &str);
    pub fn add_history(&mut self, line: &str);
    pub fn render(&self, width: u16) -> Vec<String>;
    pub fn handle_key(&mut self, key: crossterm::event::KeyEvent);
}
```

### Build and test
```
cd ~/BB-Agent && cargo build
```
