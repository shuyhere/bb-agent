use std::io;
use std::time::Duration;

use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};

use super::{
    frame::{build_frame, measure_input},
    layout::{Size, compute_layout},
    projection::{ProjectedRowKind, TranscriptProjection, TranscriptProjector},
    renderer::FullscreenRenderer,
    terminal::{FullscreenEvent, FullscreenTerminal, spawn_event_reader},
    transcript::{BlockId, BlockKind, NewBlock, Transcript},
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
            status_line:
                "Ctrl+O transcript • Enter submits • Shift+Enter inserts a newline • wheel scrolls transcript"
                    .to_string(),
            transcript: Transcript::new(),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct FullscreenOutcome {
    pub submitted_inputs: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FullscreenMode {
    #[default]
    Normal,
    Transcript,
    Search,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FullscreenSearchState {
    pub query: String,
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
    pub mode: FullscreenMode,
    pub focused_block: Option<BlockId>,
    pub search: FullscreenSearchState,
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
            mode: FullscreenMode::Normal,
            focused_block: None,
            search: FullscreenSearchState::default(),
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
        self.status_line = format!("resized to {}x{} • {}", width, height, self.mode_help_text());
        self.refresh_projection(true);
        self.dirty = true;
    }

    pub fn on_paste(&mut self, text: &str) {
        match self.mode {
            FullscreenMode::Normal => {
                self.insert_str(text);
                self.status_line = format!("pasted {} bytes", text.len());
            }
            FullscreenMode::Search => {
                self.search.query.push_str(text);
                self.status_line = format!("search {}", self.search_prompt());
                self.dirty = true;
            }
            FullscreenMode::Transcript => {
                self.status_line = "paste is ignored while transcript navigation is active"
                    .to_string();
                self.dirty = true;
            }
        }
    }

    pub fn on_key(&mut self, key: KeyEvent) {
        if matches!(key.kind, KeyEventKind::Release) {
            return;
        }

        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.should_quit = true;
                self.dirty = true;
                return;
            }
            KeyCode::Char('q') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.should_quit = true;
                self.dirty = true;
                return;
            }
            KeyCode::Char('o') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.toggle_transcript_mode();
                return;
            }
            _ => {}
        }

        match self.mode {
            FullscreenMode::Normal => self.on_normal_key(key),
            FullscreenMode::Transcript => self.on_transcript_key(key),
            FullscreenMode::Search => self.on_search_key(key),
        }
    }

    pub fn on_mouse(&mut self, event: MouseEvent) {
        let layout = self.current_layout();
        let in_transcript = event.row >= layout.transcript.y
            && event.row < layout.transcript.y.saturating_add(layout.transcript.height);

        match event.kind {
            MouseEventKind::ScrollUp if in_transcript => {
                self.viewport.scroll_up(3);
                self.mode = FullscreenMode::Transcript;
                self.focus_first_visible_block();
                self.status_line = format!(
                    "transcript row {} • j/k navigate • Enter toggles",
                    self.viewport.viewport_top
                );
                self.dirty = true;
            }
            MouseEventKind::ScrollDown if in_transcript => {
                self.viewport.scroll_down(3);
                self.mode = FullscreenMode::Transcript;
                self.focus_first_visible_block();
                self.status_line = format!(
                    "transcript row {} • j/k navigate • Enter toggles",
                    self.viewport.viewport_top
                );
                self.dirty = true;
            }
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(block_id) = self.header_block_at_screen_row(event.row) {
                    self.mode = FullscreenMode::Transcript;
                    self.set_focused_block(Some(block_id));
                    self.toggle_block(block_id);
                }
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
        let layout = self.current_layout();
        let anchor = if preserve_anchor && !self.viewport.auto_follow {
            if let Some(block_id) = self.focused_block {
                self.viewport.capture_header_anchor(&self.projection, block_id)
            } else {
                self.viewport.capture_top_anchor(&self.projection)
            }
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
        self.focused_block = self
            .focused_block
            .filter(|block_id| self.projection.rows_for_block(*block_id).is_some());
        if matches!(self.mode, FullscreenMode::Transcript | FullscreenMode::Search)
            && self.focused_block.is_none()
        {
            self.focused_block = self.default_focus_block();
        }
        self.sync_focus_tracking();
        if matches!(self.mode, FullscreenMode::Transcript | FullscreenMode::Search) {
            self.ensure_focus_visible();
        }
    }

    fn on_normal_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
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

    fn on_transcript_key(&mut self, key: KeyEvent) {
        match (key.code, key.modifiers) {
            (KeyCode::Esc, KeyModifiers::NONE) => {
                self.mode = FullscreenMode::Normal;
                self.status_line = self.mode_help_text();
                self.dirty = true;
            }
            (KeyCode::Down, KeyModifiers::NONE) | (KeyCode::Char('j'), KeyModifiers::NONE) => {
                self.move_focus(1);
            }
            (KeyCode::Up, KeyModifiers::NONE) | (KeyCode::Char('k'), KeyModifiers::NONE) => {
                self.move_focus(-1);
            }
            (KeyCode::PageDown, KeyModifiers::NONE) => {
                self.page_move(1);
            }
            (KeyCode::PageUp, KeyModifiers::NONE) => {
                self.page_move(-1);
            }
            (KeyCode::Home, KeyModifiers::NONE) | (KeyCode::Char('g'), KeyModifiers::NONE) => {
                self.focus_first();
            }
            (KeyCode::End, KeyModifiers::NONE) | (KeyCode::Char('G'), KeyModifiers::SHIFT) => {
                self.focus_last();
            }
            (KeyCode::Enter, KeyModifiers::NONE)
            | (KeyCode::Char(' '), KeyModifiers::NONE) => {
                self.toggle_focused_block();
            }
            (KeyCode::Char('o'), KeyModifiers::NONE) => {
                self.expand_focused_block();
            }
            (KeyCode::Char('c'), KeyModifiers::NONE) => {
                self.collapse_focused_block();
            }
            (KeyCode::Char('/'), KeyModifiers::NONE) => {
                self.mode = FullscreenMode::Search;
                self.search.query.clear();
                self.status_line = format!("search {}", self.search_prompt());
                self.dirty = true;
            }
            (KeyCode::Char('n'), KeyModifiers::NONE) => {
                self.search_step(true);
            }
            (KeyCode::Char('N'), KeyModifiers::SHIFT) => {
                self.search_step(false);
            }
            _ => {}
        }
    }

    fn on_search_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc if key.modifiers == KeyModifiers::NONE => {
                self.mode = FullscreenMode::Transcript;
                self.status_line = self.mode_help_text();
                self.dirty = true;
            }
            KeyCode::Enter if key.modifiers == KeyModifiers::NONE => {
                self.mode = FullscreenMode::Transcript;
                if self.search.query.trim().is_empty() {
                    self.status_line = "search scaffold ready • type after / to filter transcript"
                        .to_string();
                    self.dirty = true;
                } else {
                    self.search_step(true);
                }
            }
            KeyCode::Backspace if key.modifiers == KeyModifiers::NONE => {
                self.search.query.pop();
                self.status_line = format!("search {}", self.search_prompt());
                self.dirty = true;
            }
            KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.search.query.push(ch);
                self.status_line = format!("search {}", self.search_prompt());
                self.dirty = true;
            }
            _ => {}
        }
    }

    fn toggle_transcript_mode(&mut self) {
        self.mode = match self.mode {
            FullscreenMode::Normal => FullscreenMode::Transcript,
            FullscreenMode::Transcript | FullscreenMode::Search => FullscreenMode::Normal,
        };

        if matches!(self.mode, FullscreenMode::Transcript | FullscreenMode::Search)
            && self.focused_block.is_none()
        {
            self.focused_block = self.default_focus_block();
            self.sync_focus_tracking();
            self.ensure_focus_visible();
        }

        self.status_line = self.mode_help_text();
        self.dirty = true;
    }

    fn mode_help_text(&self) -> String {
        match self.mode {
            FullscreenMode::Normal => {
                "normal mode • Ctrl+O transcript • Enter submits • Shift+Enter inserts a newline • Esc quits"
                    .to_string()
            }
            FullscreenMode::Transcript => {
                "transcript mode • j/k navigate • Enter/Space toggle • o expand • c collapse • / search • Esc returns"
                    .to_string()
            }
            FullscreenMode::Search => {
                format!(
                    "search mode • type to filter • Enter jumps • Esc returns • {}",
                    self.search_prompt()
                )
            }
        }
    }

    fn search_prompt(&self) -> String {
        if self.search.query.is_empty() {
            "/".to_string()
        } else {
            format!("/{}", self.search.query)
        }
    }

    fn current_layout(&self) -> super::layout::FullscreenLayout {
        let input_inner_width = self.size.width.saturating_sub(2).max(1) as usize;
        let input_wrap = measure_input(&self.input, self.cursor, input_inner_width);
        compute_layout(self.size, input_wrap.lines.len())
    }

    fn sync_focus_tracking(&mut self) {
        self.viewport.selected_block = self.focused_block;
        self.viewport.focused_row = self
            .focused_block
            .and_then(|block_id| self.projection.header_row_for_block(block_id));
    }

    fn focus_first_visible_block(&mut self) {
        let visible = self.visible_header_blocks();
        if let Some(block_id) = visible.first().copied() {
            self.set_focused_block(Some(block_id));
        }
    }

    fn default_focus_block(&self) -> Option<BlockId> {
        self.visible_header_blocks()
            .last()
            .copied()
            .or_else(|| self.last_focusable_block())
            .or_else(|| self.first_focusable_block())
    }

    fn visible_header_blocks(&self) -> Vec<BlockId> {
        self.viewport
            .visible_row_range()
            .filter_map(|row_index| {
                let row = self.projection.row(row_index)?;
                if row.kind != ProjectedRowKind::Header
                    || self.projection.header_row_for_block(row.block_id) != Some(row.index)
                {
                    return None;
                }
                Some(row.block_id)
            })
            .collect()
    }

    fn focusable_blocks(&self) -> Vec<BlockId> {
        self.projection
            .rows
            .iter()
            .filter(|row| {
                row.kind == ProjectedRowKind::Header
                    && self.projection.header_row_for_block(row.block_id) == Some(row.index)
            })
            .map(|row| row.block_id)
            .collect()
    }

    fn first_focusable_block(&self) -> Option<BlockId> {
        self.focusable_blocks().into_iter().next()
    }

    fn last_focusable_block(&self) -> Option<BlockId> {
        self.focusable_blocks().into_iter().last()
    }

    fn set_focused_block(&mut self, block_id: Option<BlockId>) {
        self.focused_block = block_id;
        self.sync_focus_tracking();
        self.dirty = true;
    }

    fn focus_block(&mut self, block_id: BlockId) {
        self.focused_block = Some(block_id);
        self.sync_focus_tracking();
        self.ensure_focus_visible();
        self.dirty = true;
    }

    fn focus_first(&mut self) {
        if let Some(block_id) = self.first_focusable_block() {
            self.focus_block(block_id);
        }
    }

    fn focus_last(&mut self) {
        if let Some(block_id) = self.last_focusable_block() {
            self.focus_block(block_id);
        }
    }

    fn move_focus(&mut self, step: isize) {
        let blocks = self.focusable_blocks();
        if blocks.is_empty() {
            return;
        }

        let current_index = self
            .focused_block
            .and_then(|block_id| blocks.iter().position(|candidate| *candidate == block_id))
            .unwrap_or_else(|| if step.is_negative() { blocks.len() - 1 } else { 0 });

        let next_index = if step.is_negative() {
            current_index.saturating_sub(step.unsigned_abs())
        } else {
            (current_index + step as usize).min(blocks.len().saturating_sub(1))
        };

        self.focus_block(blocks[next_index]);
    }

    fn page_move(&mut self, direction: isize) {
        let step = self.visible_header_blocks().len().max(1);
        for _ in 0..step {
            self.move_focus(direction.signum());
        }
    }

    fn ensure_focus_visible(&mut self) {
        let Some(block_id) = self.focused_block else {
            self.sync_focus_tracking();
            return;
        };
        let Some(header_row) = self.projection.header_row_for_block(block_id) else {
            self.sync_focus_tracking();
            return;
        };

        if self.viewport.viewport_height == 0 {
            self.sync_focus_tracking();
            return;
        }

        if header_row < self.viewport.viewport_top {
            self.viewport.viewport_top = header_row;
            self.viewport.auto_follow = false;
        } else if header_row >= self.viewport.viewport_top + self.viewport.viewport_height {
            self.viewport.viewport_top = header_row
                .saturating_add(1)
                .saturating_sub(self.viewport.viewport_height);
            self.viewport.auto_follow = false;
        }
        self.sync_focus_tracking();
    }

    fn search_step(&mut self, forward: bool) {
        let query = self.search.query.trim().to_ascii_lowercase();
        if query.is_empty() {
            self.status_line =
                "search scaffold ready • press / and type to jump between transcript blocks"
                    .to_string();
            self.dirty = true;
            return;
        }

        let blocks = self.focusable_blocks();
        if blocks.is_empty() {
            return;
        }

        let current = self
            .focused_block
            .and_then(|block_id| blocks.iter().position(|candidate| *candidate == block_id))
            .unwrap_or(0);

        for offset in 1..=blocks.len() {
            let index = if forward {
                (current + offset) % blocks.len()
            } else {
                (current + blocks.len() - (offset % blocks.len())) % blocks.len()
            };
            let block_id = blocks[index];
            if self.block_matches_query(block_id, &query) {
                self.focus_block(block_id);
                self.status_line = format!("matched {}", self.search_prompt());
                return;
            }
        }

        self.status_line = format!("no matches for {}", self.search_prompt());
        self.dirty = true;
    }

    fn block_matches_query(&self, block_id: BlockId, query: &str) -> bool {
        let Some(block) = self.transcript.block(block_id) else {
            return false;
        };
        format!("{}\n{}", block.title, block.content)
            .to_ascii_lowercase()
            .contains(query)
    }

    fn toggle_focused_block(&mut self) {
        let Some(block_id) = self.focused_block else {
            return;
        };
        self.toggle_block(block_id);
    }

    fn expand_focused_block(&mut self) {
        let Some(block_id) = self.focused_block else {
            return;
        };
        self.set_block_collapsed(block_id, false, "expanded");
    }

    fn collapse_focused_block(&mut self) {
        let Some(block_id) = self.focused_block else {
            return;
        };
        self.set_block_collapsed(block_id, true, "collapsed");
    }

    fn toggle_block(&mut self, block_id: BlockId) {
        let Some(block) = self.transcript.block(block_id).cloned() else {
            return;
        };
        if !(block.expandable || !block.children.is_empty()) {
            self.status_line = format!("focused {} block is not expandable", self.block_label(block_id));
            self.dirty = true;
            return;
        }

        let next_collapsed = !block.collapsed;
        let action = if next_collapsed { "collapsed" } else { "expanded" };
        self.set_block_collapsed(block_id, next_collapsed, action);
    }

    fn set_block_collapsed(&mut self, block_id: BlockId, collapsed: bool, action: &str) {
        let Some(block) = self.transcript.block(block_id).cloned() else {
            return;
        };
        if !(block.expandable || !block.children.is_empty()) {
            self.status_line = format!("focused {} block is not expandable", self.block_label(block_id));
            self.dirty = true;
            return;
        }
        if block.collapsed == collapsed {
            self.status_line = format!("{} already {}", self.block_label(block_id), action);
            self.dirty = true;
            return;
        }

        if self.transcript.set_collapsed(block_id, collapsed).is_err() {
            return;
        }

        self.refresh_projection(true);
        self.focus_block(block_id);
        self.status_line = format!("{} {}", action, self.block_label(block_id));
    }

    fn block_label(&self, block_id: BlockId) -> String {
        self.transcript
            .block(block_id)
            .map(|block| {
                if block.title.trim().is_empty() {
                    format!("block {}", block_id.get())
                } else {
                    format!("“{}”", block.title)
                }
            })
            .unwrap_or_else(|| format!("block {}", block_id.get()))
    }

    fn header_block_at_screen_row(&self, row: u16) -> Option<BlockId> {
        let layout = self.current_layout();
        if row < layout.transcript.y || row >= layout.transcript.y + layout.transcript.height {
            return None;
        }

        let visible = self.viewport.visible_row_range();
        let visible_count = visible.end.saturating_sub(visible.start);
        let top_padding = (layout.transcript.height as usize).saturating_sub(visible_count);
        let local_row = row.saturating_sub(layout.transcript.y) as usize;
        if local_row < top_padding {
            return None;
        }

        let projected_row = visible.start + local_row.saturating_sub(top_padding);
        let row = self.projection.row(projected_row)?;
        if row.kind != ProjectedRowKind::Header {
            return None;
        }
        Some(row.block_id)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_state() -> (FullscreenState, BlockId, BlockId, BlockId) {
        let mut transcript = Transcript::new();
        let intro = transcript.append_root_block(
            NewBlock::new(BlockKind::SystemNote, "intro").with_content("foundation"),
        );
        let tool = transcript.append_root_block(
            NewBlock::new(BlockKind::ToolUse, "read config")
                .with_content("read /tmp/demo.txt")
                .with_expandable(true),
        );
        let result = transcript
            .append_child_block(
                tool,
                NewBlock::new(BlockKind::ToolResult, "output").with_content("hello world"),
            )
            .expect("tool result should be appended");

        let state = FullscreenState::new(
            FullscreenAppConfig {
                transcript,
                ..FullscreenAppConfig::default()
            },
            Size {
                width: 80,
                height: 12,
            },
        );
        (state, intro, tool, result)
    }

    #[test]
    fn ctrl_o_and_escape_switch_modes_before_quitting() {
        let (mut state, _, _, _) = sample_state();

        state.on_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL));
        assert_eq!(state.mode, FullscreenMode::Transcript);
        assert!(state.focused_block.is_some());
        assert!(!state.should_quit);

        state.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(state.mode, FullscreenMode::Normal);
        assert!(!state.should_quit);

        state.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(state.should_quit);
    }

    #[test]
    fn transcript_keys_navigate_and_toggle_expansion() {
        let (mut state, intro, tool, _) = sample_state();

        state.on_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL));
        state.focused_block = Some(intro);
        state.sync_focus_tracking();

        state.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(state.focused_block, Some(tool));

        state.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(state.transcript.block(tool).expect("tool block").collapsed);

        state.on_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::NONE));
        assert!(!state.transcript.block(tool).expect("tool block").collapsed);

        state.on_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));
        assert!(state.transcript.block(tool).expect("tool block").collapsed);
    }

    #[test]
    fn mouse_click_on_header_toggles_block() {
        let (mut state, _, tool, _) = sample_state();
        let header_row = state
            .projection
            .header_row_for_block(tool)
            .expect("header row should exist");
        let local_row = header_row.saturating_sub(state.viewport.viewport_top);
        let layout = state.current_layout();
        let visible = state.viewport.visible_row_range();
        let top_padding = (layout.transcript.height as usize)
            .saturating_sub(visible.end.saturating_sub(visible.start));
        let screen_row = layout.transcript.y + (top_padding + local_row) as u16;

        state.on_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 0,
            row: screen_row,
            modifiers: KeyModifiers::NONE,
        });

        assert_eq!(state.mode, FullscreenMode::Transcript);
        assert_eq!(state.focused_block, Some(tool));
        assert!(state.transcript.block(tool).expect("tool block").collapsed);
    }

    #[test]
    fn search_step_moves_focus_to_matching_block() {
        let (mut state, intro, _, result) = sample_state();

        state.on_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL));
        state.focused_block = Some(intro);
        state.sync_focus_tracking();
        state.search.query = "world".to_string();
        state.search_step(true);

        assert_eq!(state.focused_block, Some(result));
    }
}
