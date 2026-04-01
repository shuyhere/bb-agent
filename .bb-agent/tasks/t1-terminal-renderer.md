Build the Terminal abstraction and differential renderer for BB-Agent TUI.

Work in `~/BB-Agent/crates/tui/src/`. Read AGENTS.md for project context.

## Task: Build these files

### 1. `terminal.rs` — Terminal trait + ProcessTerminal

```rust
pub trait Terminal {
    fn start(&mut self);  // enable raw mode, bracketed paste
    fn stop(&mut self);   // restore terminal state
    fn write(&mut self, data: &str);
    fn columns(&self) -> u16;
    fn rows(&self) -> u16;
    fn hide_cursor(&mut self);
    fn show_cursor(&mut self);
}

pub struct ProcessTerminal { ... }
```

Use crossterm for:
- `terminal::enable_raw_mode()` / `disable_raw_mode()`
- `execute!(stdout, cursor::Hide)` etc
- Synchronized output: write `\x1b[?2026h` before and `\x1b[?2026l` after render
- Get size via `terminal::size()`

### 2. `component.rs` — Component trait + Container

```rust
pub trait Component: Send {
    fn render(&self, width: u16) -> Vec<String>;
    fn handle_input(&mut self, _data: &crossterm::event::KeyEvent) {}
    fn invalidate(&mut self) {}
}

pub struct Container {
    pub children: Vec<Box<dyn Component>>,
}
```

Container renders all children vertically (concatenate lines).

### 3. `renderer.rs` — Differential renderer

```rust
pub struct DiffRenderer {
    previous_lines: Vec<String>,
    previous_width: u16,
}

impl DiffRenderer {
    pub fn render(&mut self, new_lines: &[String], terminal: &mut dyn Terminal);
}
```

Algorithm:
1. First render: output all lines
2. Width changed: clear screen, output all
3. Normal: find first changed line, move cursor there, re-render from that point
4. Wrap all output in synchronized output escape sequences
5. Handle content shrinking (clear extra lines)

### 4. `utils.rs` — ANSI-aware string utilities

```rust
pub fn visible_width(s: &str) -> usize;       // width excluding ANSI escapes
pub fn truncate_to_width(s: &str, max: usize) -> String;
pub fn pad_to_width(s: &str, width: usize) -> String;
```

Use `unicode-width` crate for correct CJK/emoji handling.
Strip ANSI escape sequences before measuring width.

### 5. Update `lib.rs`

```rust
pub mod terminal;
pub mod component;
pub mod renderer;
pub mod utils;
pub mod chat;
pub mod editor;
pub mod status;
pub mod app;
```

## Build and test
```
cd ~/BB-Agent && cargo build
```

Make sure the existing `chat.rs`, `editor.rs`, `status.rs`, `app.rs` still compile.
You may need to update them to use the new types.
