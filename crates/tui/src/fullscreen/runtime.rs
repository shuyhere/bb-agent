use std::io;
use std::time::Duration;

use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};

use super::{
    frame::{build_frame, measure_input},
    layout::{Size, compute_layout},
    projection::{TranscriptProjection, TranscriptProjector},
    renderer::FullscreenRenderer,
    terminal::{FullscreenEvent, FullscreenTerminal, spawn_event_reader},
    transcript::{BlockKind, NewBlock, Transcript},
    viewport::ViewportState,
};

#[derive(Clone, Debug)]
pub struct FullscreenAppConfig {
    pub title: String,
    pub input_placeholder: String,
    pub status_line: String,
    pub transcript: Transcript,
}

impl Default for FullscreenAppConfig {
    fn default() -> Self {
        Self {
            title: "BB-Agent fullscreen transcript".to_string(),
            input_placeholder: "Type a prompt…".to_string(),
            status_line: "Esc quits • Enter submits • Shift+Enter inserts a newline • wheel scrolls transcript"
                .to_string(),
            transcript: Transcript::new(),
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
    pub transcript: Transcript,
    pub input: String,
    pub cursor: usize,
    pub size: Size,
    pub viewport: ViewportState,
    pub projection: TranscriptProjection,
    pub dirty: bool,
    pub should_quit: bool,
    pub tick_count: u64,
    pub submitted_inputs: Vec<String>,
}

impl FullscreenState {
    pub fn new(config: FullscreenAppConfig, size: Size) -> Self {
        let mut state = Self {
            title: config.title,
            input_placeholder: config.input_placeholder,
            status_line: config.status_line,
            transcript: config.transcript,
            input: String::new(),
            cursor: 0,
            size,
            viewport: ViewportState::new(0),
            projection: TranscriptProjection::default(),
            dirty: true,
            should_quit: false,
            tick_count: 0,
            submitted_inputs: Vec::new(),
        };
        state.refresh_projection(false);
        state
    }

    pub fn take_dirty(&mut self) -> bool {
        let dirty = self.dirty;
        self.dirty = false;
        dirty
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
        self.refresh_projection(true);
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
            KeyCode::PageUp => {
                let page = self.viewport.viewport_height.saturating_sub(1).max(1);
                self.viewport.scroll_up(page);
                self.dirty = true;
            }
            KeyCode::PageDown => {
                let page = self.viewport.viewport_height.saturating_sub(1).max(1);
                self.viewport.scroll_down(page);
                self.dirty = true;
            }
            KeyCode::Char('g') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.viewport.jump_to_bottom();
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
                self.viewport.scroll_up(3);
                self.status_line = format!("transcript row {}", self.viewport.viewport_top);
                self.dirty = true;
            }
            MouseEventKind::ScrollDown => {
                self.viewport.scroll_down(3);
                self.status_line = format!("transcript row {}", self.viewport.viewport_top);
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

    pub fn refresh_projection(&mut self, preserve_anchor: bool) {
        let input_inner_width = self.size.width.saturating_sub(2).max(1) as usize;
        let input_wrap = measure_input(&self.input, self.cursor, input_inner_width);
        let layout = compute_layout(self.size, input_wrap.lines.len());

        let anchor = if preserve_anchor && !self.viewport.auto_follow {
            self.viewport.capture_top_anchor(&self.projection)
        } else {
            None
        };

        let mut projector = TranscriptProjector::new();
        let next_projection = projector.project(&self.transcript, layout.transcript.width as usize);
        self.viewport
            .set_viewport_height(layout.transcript.height as usize);
        if let Some(anchor) = anchor {
            self.viewport.preserve_anchor(&next_projection, &anchor);
        } else {
            self.viewport.on_projection_changed(&next_projection);
        }
        self.projection = next_projection;
    }

    fn submit_input(&mut self) {
        let submitted = self.input.trim_end().to_string();
        if submitted.trim().is_empty() {
            self.status_line = "empty input ignored".to_string();
            self.dirty = true;
            return;
        }

        self.transcript.append_root_block(
            NewBlock::new(BlockKind::UserMessage, "prompt").with_content(submitted.clone()),
        );
        self.submitted_inputs.push(submitted.clone());
        self.input.clear();
        self.cursor = 0;
        self.status_line = format!(
            "captured prompt locally ({} chars) • agent turn wiring lands in a later branch",
            submitted.chars().count()
        );
        self.refresh_projection(true);
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

pub async fn run(config: FullscreenAppConfig) -> io::Result<FullscreenOutcome> {
    let mut terminal = FullscreenTerminal::enter()?;
    let (width, height) = terminal.size()?;
    let mut state = FullscreenState::new(config, Size { width, height });
    let mut renderer = FullscreenRenderer::new();
    let mut events = spawn_event_reader();
    let mut tick = tokio::time::interval(Duration::from_millis(100));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    if state.take_dirty() {
        let frame = build_frame(&state);
        renderer.render(&mut terminal, &frame)?;
    }

    loop {
        if state.should_quit {
            break;
        }

        tokio::select! {
            maybe_event = events.recv() => {
                let Some(event) = maybe_event else {
                    break;
                };

                match event {
                    FullscreenEvent::Key(key) => state.on_key(key),
                    FullscreenEvent::Mouse(mouse) => state.on_mouse(mouse),
                    FullscreenEvent::Resize(width, height) => state.on_resize(width, height),
                    FullscreenEvent::Paste(text) => state.on_paste(&text),
                }
            }
            _ = tick.tick() => {
                state.on_tick();
            }
        }

        if state.take_dirty() {
            let frame = build_frame(&state);
            renderer.render(&mut terminal, &frame)?;
        }
    }

    Ok(state.outcome())
}

fn previous_boundary(text: &str, cursor: usize) -> usize {
    text[..cursor]
        .char_indices()
        .next_back()
        .map(|(idx, _)| idx)
        .unwrap_or(0)
}

fn next_boundary(text: &str, cursor: usize) -> usize {
    text[cursor..]
        .char_indices()
        .nth(1)
        .map(|(idx, _)| cursor + idx)
        .unwrap_or(text.len())
}
