use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};

use super::layout::Size;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TranscriptRole {
    System,
    User,
    Assistant,
    Status,
}

impl TranscriptRole {
    pub fn label(&self) -> &'static str {
        match self {
            Self::System => "system",
            Self::User => "you",
            Self::Assistant => "bb",
            Self::Status => "status",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TranscriptItem {
    pub role: TranscriptRole,
    pub text: String,
}

impl TranscriptItem {
    pub fn new(role: TranscriptRole, text: impl Into<String>) -> Self {
        Self {
            role,
            text: text.into(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct FullscreenAppConfig {
    pub title: String,
    pub input_placeholder: String,
    pub status_line: String,
    pub transcript: Vec<TranscriptItem>,
}

impl Default for FullscreenAppConfig {
    fn default() -> Self {
        Self {
            title: "BB-Agent fullscreen transcript".to_string(),
            input_placeholder: "Type a prompt…".to_string(),
            status_line: "Esc quits • Enter submits • Shift+Enter inserts a newline • wheel scrolls transcript"
                .to_string(),
            transcript: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct FullscreenOutcome {
    pub submitted_inputs: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct FullscreenState {
    pub title: String,
    pub input_placeholder: String,
    pub status_line: String,
    pub transcript: Vec<TranscriptItem>,
    pub input: String,
    pub cursor: usize,
    pub size: Size,
    pub transcript_scroll: usize,
    pub dirty: bool,
    pub should_quit: bool,
    pub tick_count: u64,
    pub submitted_inputs: Vec<String>,
}

impl FullscreenState {
    pub fn new(config: FullscreenAppConfig, size: Size) -> Self {
        Self {
            title: config.title,
            input_placeholder: config.input_placeholder,
            status_line: config.status_line,
            transcript: config.transcript,
            input: String::new(),
            cursor: 0,
            size,
            transcript_scroll: 0,
            dirty: true,
            should_quit: false,
            tick_count: 0,
            submitted_inputs: Vec::new(),
        }
    }

    pub fn take_dirty(&mut self) -> bool {
        let dirty = self.dirty;
        self.dirty = false;
        dirty
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    pub fn on_tick(&mut self) {
        self.tick_count = self.tick_count.wrapping_add(1);
        if self.tick_count % 8 == 0 {
            self.dirty = true;
        }
    }

    pub fn on_resize(&mut self, width: u16, height: u16) {
        self.size = Size { width, height };
        self.status_line = format!(
            "resized to {}x{} • Esc quits • Enter submits • Shift+Enter inserts a newline • wheel scrolls transcript",
            width, height
        );
        self.dirty = true;
    }

    pub fn on_paste(&mut self, text: &str) {
        self.insert_str(text);
        self.status_line = format!("pasted {} bytes", text.len());
        self.dirty = true;
    }

    pub fn on_key(&mut self, key: KeyEvent) {
        if matches!(key.kind, KeyEventKind::Release) {
            return;
        }

        match key.code {
            KeyCode::Esc => {
                self.should_quit = true;
                self.dirty = true;
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.should_quit = true;
                self.dirty = true;
            }
            KeyCode::Char('q') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.should_quit = true;
                self.dirty = true;
            }
            KeyCode::Enter if key.modifiers.contains(KeyModifiers::SHIFT) => {
                self.insert_char('\n');
            }
            KeyCode::Enter => {
                self.submit_input();
            }
            KeyCode::Backspace => {
                self.backspace();
            }
            KeyCode::Left => {
                self.move_left();
            }
            KeyCode::Right => {
                self.move_right();
            }
            KeyCode::Home => {
                self.cursor = 0;
                self.dirty = true;
            }
            KeyCode::End => {
                self.cursor = self.input.len();
                self.dirty = true;
            }
            KeyCode::Char(ch) if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.status_line = format!("ignored Ctrl+{ch}");
                self.dirty = true;
            }
            KeyCode::Char(ch) => {
                self.insert_char(ch);
            }
            _ => {}
        }
    }

    pub fn on_mouse(&mut self, event: MouseEvent) {
        match event.kind {
            MouseEventKind::ScrollUp => {
                self.transcript_scroll = self.transcript_scroll.saturating_add(3);
                self.status_line = format!("transcript scroll {}", self.transcript_scroll);
                self.dirty = true;
            }
            MouseEventKind::ScrollDown => {
                self.transcript_scroll = self.transcript_scroll.saturating_sub(3);
                self.status_line = format!("transcript scroll {}", self.transcript_scroll);
                self.dirty = true;
            }
            MouseEventKind::Down(MouseButton::Left) => {
                self.status_line = format!("mouse click at {},{}", event.column, event.row);
                self.dirty = true;
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                self.status_line = format!("mouse drag at {},{}", event.column, event.row);
                self.dirty = true;
            }
            _ => {}
        }
    }

    pub fn outcome(&self) -> FullscreenOutcome {
        FullscreenOutcome {
            submitted_inputs: self.submitted_inputs.clone(),
        }
    }

    fn submit_input(&mut self) {
        let submitted = self.input.trim_end().to_string();
        if submitted.trim().is_empty() {
            self.status_line = "empty input ignored".to_string();
            self.dirty = true;
            return;
        }

        self.transcript
            .push(TranscriptItem::new(TranscriptRole::User, submitted.clone()));
        self.submitted_inputs.push(submitted.clone());
        self.input.clear();
        self.cursor = 0;
        self.transcript_scroll = 0;
        self.status_line = format!(
            "captured prompt locally ({} chars) • agent turn wiring lands in a later branch",
            submitted.chars().count()
        );
        self.dirty = true;
    }

    fn insert_char(&mut self, ch: char) {
        self.input.insert(self.cursor, ch);
        self.cursor += ch.len_utf8();
        self.dirty = true;
    }

    fn insert_str(&mut self, text: &str) {
        self.input.insert_str(self.cursor, text);
        self.cursor += text.len();
        self.dirty = true;
    }

    fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let prev = previous_boundary(&self.input, self.cursor);
        self.input.drain(prev..self.cursor);
        self.cursor = prev;
        self.dirty = true;
    }

    fn move_left(&mut self) {
        if self.cursor == 0 {
            return;
        }
        self.cursor = previous_boundary(&self.input, self.cursor);
        self.dirty = true;
    }

    fn move_right(&mut self) {
        if self.cursor >= self.input.len() {
            return;
        }
        self.cursor = next_boundary(&self.input, self.cursor);
        self.dirty = true;
    }
}

fn previous_boundary(text: &str, cursor: usize) -> usize {
    text[..cursor]
        .char_indices()
        .last()
        .map(|(idx, _)| idx)
        .unwrap_or(0)
}

fn next_boundary(text: &str, cursor: usize) -> usize {
    let mut iter = text[cursor..].char_indices();
    let Some((_, ch)) = iter.next() else {
        return text.len();
    };
    cursor + ch.len_utf8()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn submit_moves_input_into_transcript() {
        let mut state = FullscreenState::new(
            FullscreenAppConfig::default(),
            Size {
                width: 80,
                height: 24,
            },
        );
        state.on_paste("hello");
        state.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert_eq!(state.transcript.len(), 1);
        assert!(state.input.is_empty());
        assert_eq!(state.submitted_inputs, vec!["hello".to_string()]);
    }

    #[test]
    fn mouse_scroll_updates_transcript_offset() {
        let mut state = FullscreenState::new(
            FullscreenAppConfig::default(),
            Size {
                width: 80,
                height: 24,
            },
        );
        state.on_mouse(MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        });

        assert_eq!(state.transcript_scroll, 3);
    }
}
