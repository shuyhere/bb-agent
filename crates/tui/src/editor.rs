use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    style::{Color, Stylize},
    terminal::{self, ClearType},
    ExecutableCommand,
};
use std::io::{self, Write};

/// Multi-line terminal editor with history, word-wrap rendering, and Emacs-style bindings.
pub struct Editor {
    /// Each line is a Vec<char>.
    lines: Vec<Vec<char>>,
    /// Cursor row (0-indexed into `lines`).
    row: usize,
    /// Cursor column (0-indexed into current line).
    col: usize,
    /// Prompt string displayed before the first line.
    prompt: String,
    /// Previously submitted inputs.
    history: Vec<String>,
    /// Current position while browsing history. `None` = editing live buffer.
    history_pos: Option<usize>,
    /// Buffer saved when the user starts browsing history.
    saved_buffer: String,
}

impl Editor {
    pub fn new(prompt: &str) -> Self {
        Self {
            lines: vec![Vec::new()],
            row: 0,
            col: 0,
            prompt: prompt.to_string(),
            history: Vec::new(),
            history_pos: None,
            saved_buffer: String::new(),
        }
    }

    // ── public helpers ───────────────────────────────────────────────

    /// Get the full text content (lines joined by '\n').
    pub fn get_text(&self) -> String {
        self.lines
            .iter()
            .map(|l| l.iter().collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Replace the buffer with `text`.
    pub fn set_text(&mut self, text: &str) {
        self.lines = text
            .split('\n')
            .map(|l| l.chars().collect())
            .collect();
        if self.lines.is_empty() {
            self.lines.push(Vec::new());
        }
        self.row = self.lines.len() - 1;
        self.col = self.lines[self.row].len();
    }

    /// Add a completed input to history (deduplicates against last entry).
    pub fn add_history(&mut self, line: &str) {
        if !line.is_empty() {
            if self.history.last().map(|l| l.as_str()) != Some(line) {
                self.history.push(line.to_string());
            }
        }
    }

    /// Render the editor content into display lines, word-wrapping to `width`.
    /// Each logical line is prefixed with the prompt (first line) or padding.
    pub fn render(&self, width: u16) -> Vec<String> {
        let w = width as usize;
        let prompt_len = self.prompt.len();
        let mut out: Vec<String> = Vec::new();

        for (i, line) in self.lines.iter().enumerate() {
            let prefix = if i == 0 {
                self.prompt.clone()
            } else {
                " ".repeat(prompt_len)
            };
            let text: String = line.iter().collect();

            if text.is_empty() {
                out.push(format!("{prefix}"));
                continue;
            }

            let avail = if w > prompt_len { w - prompt_len } else { 1 };
            let chars: Vec<char> = text.chars().collect();
            let mut pos = 0;
            let mut first = true;
            while pos < chars.len() {
                let end = (pos + avail).min(chars.len());
                let chunk: String = chars[pos..end].iter().collect();
                if first {
                    out.push(format!("{prefix}{chunk}"));
                    first = false;
                } else {
                    // continuation lines get same-width padding
                    out.push(format!("{}{}", " ".repeat(prompt_len), chunk));
                }
                pos = end;
            }
        }

        out
    }

    // ── blocking read ────────────────────────────────────────────────

    /// Read user input in raw mode. Returns `None` on Ctrl-C / Ctrl-D with
    /// empty buffer.
    pub fn read_line(&mut self) -> Option<String> {
        self.lines = vec![Vec::new()];
        self.row = 0;
        self.col = 0;
        self.history_pos = None;
        self.saved_buffer.clear();

        terminal::enable_raw_mode().ok();
        self.draw();

        let result = self.input_loop();

        terminal::disable_raw_mode().ok();
        let mut stdout = io::stdout();
        stdout.execute(cursor::MoveToColumn(0)).ok();
        println!();

        result
    }

    // ── key handling (public so callers can drive the editor externally) ──

    /// Process a single key event. Returns `Some(text)` when the user submits,
    /// `None` otherwise. A return of `Some("")` is *not* produced — empty
    /// Enter simply does nothing.
    pub fn handle_key(&mut self, key: KeyEvent) -> Option<String> {
        let KeyEvent { code, modifiers, .. } = key;

        match (code, modifiers) {
            // ── Submit (Enter) ───────────────────────────────────
            (KeyCode::Enter, KeyModifiers::NONE) => {
                let text = self.get_text();
                if text.is_empty() {
                    return None;
                }
                self.add_history(&text);
                return Some(text);
            }

            // ── Newline (Alt+Enter) ──────────────────────────────
            (KeyCode::Enter, KeyModifiers::ALT) => {
                self.insert_newline();
            }

            // ── Exit signals ─────────────────────────────────────
            (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                self.lines = vec![Vec::new()];
                self.row = 0;
                self.col = 0;
                return Some(String::new()); // signal cancel
            }
            (KeyCode::Char('d'), KeyModifiers::CONTROL) => {
                if self.get_text().is_empty() {
                    return Some(String::new());
                }
            }

            // ── Cursor movement ──────────────────────────────────
            (KeyCode::Left, KeyModifiers::NONE | KeyModifiers::SHIFT) => {
                self.move_left();
            }
            (KeyCode::Right, KeyModifiers::NONE | KeyModifiers::SHIFT) => {
                self.move_right();
            }
            (KeyCode::Up, KeyModifiers::NONE) => {
                if self.row > 0 {
                    self.row -= 1;
                    self.col = self.col.min(self.lines[self.row].len());
                } else {
                    self.history_back();
                }
            }
            (KeyCode::Down, KeyModifiers::NONE) => {
                if self.row + 1 < self.lines.len() {
                    self.row += 1;
                    self.col = self.col.min(self.lines[self.row].len());
                } else {
                    self.history_forward();
                }
            }
            (KeyCode::Home, _) | (KeyCode::Char('a'), KeyModifiers::CONTROL) => {
                self.col = 0;
            }
            (KeyCode::End, _) | (KeyCode::Char('e'), KeyModifiers::CONTROL) => {
                self.col = self.lines[self.row].len();
            }

            // Word jump: Ctrl+Left / Alt+Left
            (KeyCode::Left, KeyModifiers::CONTROL)
            | (KeyCode::Left, KeyModifiers::ALT) => {
                self.word_left();
            }
            (KeyCode::Right, KeyModifiers::CONTROL)
            | (KeyCode::Right, KeyModifiers::ALT) => {
                self.word_right();
            }

            // ── Deletion ─────────────────────────────────────────
            (KeyCode::Backspace, _) => {
                self.backspace();
            }
            (KeyCode::Delete, _) => {
                self.delete();
            }
            (KeyCode::Char('k'), KeyModifiers::CONTROL) => {
                // Kill to end of line
                self.lines[self.row].truncate(self.col);
            }
            (KeyCode::Char('u'), KeyModifiers::CONTROL) => {
                // Clear entire current line (content before cursor)
                let rest = self.lines[self.row].split_off(self.col);
                self.lines[self.row].clear();
                self.lines[self.row] = rest;
                self.col = 0;
            }
            (KeyCode::Char('w'), KeyModifiers::CONTROL) => {
                self.delete_word_backward();
            }

            // ── Regular characters ───────────────────────────────
            (KeyCode::Char(c), KeyModifiers::NONE | KeyModifiers::SHIFT) => {
                self.lines[self.row].insert(self.col, c);
                self.col += 1;
                self.history_pos = None;
            }

            _ => {}
        }

        None
    }

    // ── private: movement helpers ────────────────────────────────────

    fn move_left(&mut self) {
        if self.col > 0 {
            self.col -= 1;
        } else if self.row > 0 {
            self.row -= 1;
            self.col = self.lines[self.row].len();
        }
    }

    fn move_right(&mut self) {
        if self.col < self.lines[self.row].len() {
            self.col += 1;
        } else if self.row + 1 < self.lines.len() {
            self.row += 1;
            self.col = 0;
        }
    }

    fn word_left(&mut self) {
        // Skip whitespace, then skip word chars
        loop {
            if self.col == 0 {
                if self.row > 0 {
                    self.row -= 1;
                    self.col = self.lines[self.row].len();
                }
                break;
            }
            self.col -= 1;
            if self.col == 0 {
                break;
            }
            if !self.lines[self.row][self.col - 1].is_alphanumeric()
                && self.lines[self.row][self.col].is_alphanumeric()
            {
                break;
            }
        }
    }

    fn word_right(&mut self) {
        let line = &self.lines[self.row];
        let len = line.len();
        if self.col >= len {
            if self.row + 1 < self.lines.len() {
                self.row += 1;
                self.col = 0;
            }
            return;
        }
        // Skip current word chars
        while self.col < len && line[self.col].is_alphanumeric() {
            self.col += 1;
        }
        // Skip whitespace/punctuation
        while self.col < len && !line[self.col].is_alphanumeric() {
            self.col += 1;
        }
    }

    // ── private: editing helpers ─────────────────────────────────────

    fn insert_newline(&mut self) {
        let rest = self.lines[self.row].split_off(self.col);
        self.row += 1;
        self.lines.insert(self.row, rest);
        self.col = 0;
        self.history_pos = None;
    }

    fn backspace(&mut self) {
        if self.col > 0 {
            self.col -= 1;
            self.lines[self.row].remove(self.col);
        } else if self.row > 0 {
            // Merge current line into previous
            let current = self.lines.remove(self.row);
            self.row -= 1;
            self.col = self.lines[self.row].len();
            self.lines[self.row].extend(current);
        }
        self.history_pos = None;
    }

    fn delete(&mut self) {
        if self.col < self.lines[self.row].len() {
            self.lines[self.row].remove(self.col);
        } else if self.row + 1 < self.lines.len() {
            // Merge next line into current
            let next = self.lines.remove(self.row + 1);
            self.lines[self.row].extend(next);
        }
    }

    fn delete_word_backward(&mut self) {
        if self.col == 0 {
            return;
        }
        let line = &self.lines[self.row];
        let mut end = self.col;
        // Skip trailing whitespace
        while end > 0 && !line[end - 1].is_alphanumeric() {
            end -= 1;
        }
        // Skip word
        while end > 0 && line[end - 1].is_alphanumeric() {
            end -= 1;
        }
        self.lines[self.row].drain(end..self.col);
        self.col = end;
    }

    // ── private: history ─────────────────────────────────────────────

    fn history_back(&mut self) {
        if self.history.is_empty() {
            return;
        }
        let new_pos = match self.history_pos {
            None => {
                self.saved_buffer = self.get_text();
                self.history.len() - 1
            }
            Some(pos) => {
                if pos > 0 {
                    pos - 1
                } else {
                    return;
                }
            }
        };
        self.history_pos = Some(new_pos);
        self.set_text(&self.history[new_pos].clone());
    }

    fn history_forward(&mut self) {
        match self.history_pos {
            None => {}
            Some(pos) => {
                if pos + 1 < self.history.len() {
                    self.history_pos = Some(pos + 1);
                    let text = self.history[pos + 1].clone();
                    self.set_text(&text);
                } else {
                    self.history_pos = None;
                    let text = self.saved_buffer.clone();
                    self.set_text(&text);
                }
            }
        }
    }

    // ── private: terminal drawing ────────────────────────────────────

    fn input_loop(&mut self) -> Option<String> {
        loop {
            match event::read() {
                Ok(Event::Key(key)) => {
                    if let Some(text) = self.handle_key(key) {
                        if text.is_empty() {
                            return None;
                        }
                        return Some(text);
                    }
                    self.draw();
                }
                Ok(Event::Resize(..)) => {
                    self.draw();
                }
                Err(_) => return None,
                _ => {}
            }
        }
    }

    fn draw(&self) {
        let mut stdout = io::stdout();
        let (term_width, _) = terminal::size().unwrap_or((80, 24));

        let rendered = self.render(term_width);
        let num_lines = rendered.len();

        // Move to start, clear all editor lines, then redraw
        stdout.execute(cursor::MoveToColumn(0)).ok();

        // Clear previous lines: we always clear enough
        for _ in 0..num_lines {
            stdout
                .execute(terminal::Clear(ClearType::CurrentLine))
                .ok();
            stdout.execute(cursor::MoveDown(1)).ok();
        }
        // Move back up
        if num_lines > 0 {
            stdout.execute(cursor::MoveUp(num_lines as u16)).ok();
        }

        // Draw each rendered line
        for (i, line) in rendered.iter().enumerate() {
            stdout.execute(cursor::MoveToColumn(0)).ok();
            stdout
                .execute(terminal::Clear(ClearType::CurrentLine))
                .ok();
            // Colorize prompt portion on first line
            if i == 0 {
                let prompt_display = format!("{}", self.prompt.clone().with(Color::Blue).bold());
                let rest = &line[self.prompt.len()..];
                write!(stdout, "{}{}", prompt_display, rest).ok();
            } else {
                write!(stdout, "{}", line).ok();
            }
            if i + 1 < rendered.len() {
                write!(stdout, "\r\n").ok();
            }
        }

        // Compute cursor screen position.
        // The cursor is at (self.row, self.col) in the logical buffer.
        // We need to figure out which rendered line and column that maps to.
        let prompt_len = self.prompt.len();
        let avail = if (term_width as usize) > prompt_len {
            (term_width as usize) - prompt_len
        } else {
            1
        };

        // Count rendered lines before cursor row
        let mut rendered_line_idx = 0;
        for i in 0..self.row {
            let line_len = self.lines[i].len();
            if line_len == 0 {
                rendered_line_idx += 1;
            } else {
                rendered_line_idx += (line_len + avail - 1) / avail;
            }
        }
        // Cursor within current logical line (accounting for wrap)
        let wrap_row = self.col / avail;
        let wrap_col = self.col % avail;
        rendered_line_idx += wrap_row;

        let cursor_screen_col = prompt_len + wrap_col;

        // Move cursor from last rendered line to the target rendered line
        let last_rendered = if num_lines > 0 { num_lines - 1 } else { 0 };
        if rendered_line_idx < last_rendered {
            stdout
                .execute(cursor::MoveUp((last_rendered - rendered_line_idx) as u16))
                .ok();
        }
        stdout
            .execute(cursor::MoveToColumn(cursor_screen_col as u16))
            .ok();
        stdout.flush().ok();
    }
}
