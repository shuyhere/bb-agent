use std::collections::{HashMap, VecDeque};
use std::io;
use std::time::{Duration, Instant};

use bb_core::types::ContentBlock;
use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use tokio::sync::mpsc;

use super::{
    frame::{build_frame, measure_input},
    layout::{FullscreenLayout, Size, compute_layout},
    projection::{ProjectedRowKind, TranscriptProjection, TranscriptProjector},
    renderer::FullscreenRenderer,
    scheduler::{RenderIntent, RenderScheduler},
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

#[derive(Clone, Debug)]
pub enum FullscreenNoteLevel {
    Status,
    Warning,
    Error,
}

#[derive(Clone, Debug)]
pub enum FullscreenCommand {
    SetStatusLine(String),
    PushNote {
        level: FullscreenNoteLevel,
        text: String,
    },
    TurnStart {
        turn_index: u32,
    },
    TextDelta(String),
    ThinkingDelta(String),
    ToolCallStart {
        id: String,
        name: String,
    },
    ToolCallDelta {
        id: String,
        args: String,
    },
    ToolExecuting {
        id: String,
    },
    ToolResult {
        id: String,
        name: String,
        content: Vec<ContentBlock>,
        details: Option<serde_json::Value>,
        artifact_path: Option<String>,
        is_error: bool,
    },
    TurnEnd,
    TurnAborted,
    TurnError {
        message: String,
    },
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
struct ActiveTurnState {
    root_id: BlockId,
    turn_index: u32,
    thinking_id: Option<BlockId>,
    content_id: Option<BlockId>,
    tools: HashMap<String, ToolCallState>,
}

impl ActiveTurnState {
    fn new(root_id: BlockId, turn_index: u32) -> Self {
        Self {
            root_id,
            turn_index,
            thinking_id: None,
            content_id: None,
            tools: HashMap::new(),
        }
    }
}

#[derive(Clone, Debug)]
struct ToolCallState {
    name: String,
    tool_use_id: BlockId,
    tool_result_id: BlockId,
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
    projector: TranscriptProjector,
    projection_dirty: bool,
    pending_submissions: VecDeque<String>,
    active_turn: Option<ActiveTurnState>,
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
            projector: TranscriptProjector::new(),
            projection_dirty: true,
            pending_submissions: VecDeque::new(),
            active_turn: None,
        };
        state.prepare_for_render();
        state
    }

    pub fn take_dirty(&mut self) -> bool {
        let dirty = self.dirty;
        self.dirty = false;
        dirty
    }

    pub fn take_pending_submissions(&mut self) -> Vec<String> {
        self.pending_submissions.drain(..).collect()
    }

    pub fn on_tick(&mut self) {
        self.tick_count = self.tick_count.wrapping_add(1);
    }

    pub fn on_resize(&mut self, width: u16, height: u16) {
        self.size = Size { width, height };
        self.status_line = format!(
            "resized to {}x{} • {}",
            width,
            height,
            self.mode_help_text()
        );
        self.projection_dirty = true;
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
                self.status_line =
                    "paste is ignored while transcript navigation is active".to_string();
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

    pub fn prepare_for_render(&mut self) {
        self.refresh_projection(!self.viewport.auto_follow);
    }

    pub fn refresh_projection(&mut self, preserve_anchor: bool) {
        let layout = self.current_layout();
        let transcript_width = layout.transcript.width as usize;
        let viewport_height = layout.transcript.height as usize;
        let should_refresh = self.projection_dirty
            || self.projection.width != transcript_width
            || self.viewport.viewport_height != viewport_height;
        if !should_refresh {
            return;
        }

        let anchor = if preserve_anchor && !self.viewport.auto_follow {
            if matches!(
                self.mode,
                FullscreenMode::Transcript | FullscreenMode::Search
            ) {
                self.focused_block
                    .and_then(|block_id| {
                        self.viewport
                            .capture_header_anchor(&self.projection, block_id)
                    })
                    .or_else(|| self.viewport.capture_top_anchor(&self.projection))
            } else {
                self.viewport.capture_top_anchor(&self.projection)
            }
        } else {
            None
        };

        let next_projection = self
            .projector
            .project(&mut self.transcript, transcript_width);
        self.viewport.set_viewport_height(viewport_height);
        if let Some(anchor) = anchor {
            self.viewport.preserve_anchor(&next_projection, &anchor);
        } else {
            self.viewport.on_projection_changed(&next_projection);
        }
        self.projection = next_projection;
        self.focused_block = self
            .focused_block
            .filter(|block_id| self.projection.rows_for_block(*block_id).is_some());
        if matches!(
            self.mode,
            FullscreenMode::Transcript | FullscreenMode::Search
        ) && self.focused_block.is_none()
        {
            self.focused_block = self.default_focus_block();
        }
        self.sync_focus_tracking();
        if matches!(
            self.mode,
            FullscreenMode::Transcript | FullscreenMode::Search
        ) {
            self.ensure_focus_visible();
        }
        self.projection_dirty = false;
    }

    pub fn apply_command(&mut self, command: FullscreenCommand) -> RenderIntent {
        match command {
            FullscreenCommand::SetStatusLine(status) => {
                self.status_line = status;
                self.dirty = true;
                RenderIntent::Render
            }
            FullscreenCommand::PushNote { level, text } => {
                let title = match level {
                    FullscreenNoteLevel::Status => "status",
                    FullscreenNoteLevel::Warning => "warning",
                    FullscreenNoteLevel::Error => "error",
                };
                self.transcript.append_root_block(
                    NewBlock::new(BlockKind::SystemNote, title).with_content(text),
                );
                self.projection_dirty = true;
                self.dirty = true;
                RenderIntent::Render
            }
            FullscreenCommand::TurnStart { turn_index } => {
                let root_id = self.transcript.append_root_block(
                    NewBlock::new(
                        BlockKind::AssistantMessage,
                        format!("turn {} • streaming", turn_index + 1),
                    )
                    .with_expandable(true),
                );
                self.active_turn = Some(ActiveTurnState::new(root_id, turn_index));
                self.projection_dirty = true;
                self.dirty = true;
                RenderIntent::Render
            }
            FullscreenCommand::TextDelta(text) => {
                if text.is_empty() {
                    return RenderIntent::None;
                }
                let Ok(content_id) = self.ensure_assistant_content_block() else {
                    return RenderIntent::None;
                };
                let _ = self.transcript.append_streamed_content(content_id, text);
                self.projection_dirty = true;
                self.dirty = true;
                RenderIntent::Schedule
            }
            FullscreenCommand::ThinkingDelta(text) => {
                if text.is_empty() {
                    return RenderIntent::None;
                }
                let Ok(thinking_id) = self.ensure_thinking_block() else {
                    return RenderIntent::None;
                };
                let _ = self.transcript.append_streamed_content(thinking_id, text);
                self.projection_dirty = true;
                self.dirty = true;
                RenderIntent::Schedule
            }
            FullscreenCommand::ToolCallStart { id, name } => {
                let Some(turn_root_id) = self.ensure_active_turn_root() else {
                    return RenderIntent::None;
                };
                let Ok(tool_use_id) = self.transcript.append_child_block(
                    turn_root_id,
                    NewBlock::new(BlockKind::ToolUse, format!("{name} • collecting"))
                        .with_expandable(true),
                ) else {
                    return RenderIntent::None;
                };
                let Ok(tool_result_id) = self.transcript.append_child_block(
                    tool_use_id,
                    NewBlock::new(BlockKind::ToolResult, "pending"),
                ) else {
                    return RenderIntent::None;
                };
                if let Some(active_turn) = self.active_turn.as_mut() {
                    active_turn.tools.insert(
                        id,
                        ToolCallState {
                            name,
                            tool_use_id,
                            tool_result_id,
                        },
                    );
                }
                self.projection_dirty = true;
                self.dirty = true;
                RenderIntent::Render
            }
            FullscreenCommand::ToolCallDelta { id, args } => {
                if args.is_empty() {
                    return RenderIntent::None;
                }
                let Some(tool) = self.tool_call_state(&id).cloned() else {
                    return RenderIntent::None;
                };
                let _ = self
                    .transcript
                    .append_streamed_content(tool.tool_use_id, args);
                self.projection_dirty = true;
                self.dirty = true;
                RenderIntent::Schedule
            }
            FullscreenCommand::ToolExecuting { id } => {
                let Some(tool) = self.tool_call_state(&id).cloned() else {
                    return RenderIntent::None;
                };
                let _ = self
                    .transcript
                    .update_title(tool.tool_use_id, format!("{} • running", tool.name));
                let _ = self.transcript.update_title(tool.tool_result_id, "pending");
                self.projection_dirty = true;
                self.dirty = true;
                RenderIntent::Render
            }
            FullscreenCommand::ToolResult {
                id,
                name,
                content,
                details,
                artifact_path,
                is_error,
            } => {
                let Some(tool) = self.tool_call_state(&id).cloned() else {
                    return RenderIntent::None;
                };
                let _ = self.transcript.update_title(
                    tool.tool_use_id,
                    format!("{} • {}", name, if is_error { "error" } else { "done" }),
                );
                let _ = self.transcript.update_title(
                    tool.tool_result_id,
                    if is_error { "error" } else { "result" },
                );
                let formatted = format_tool_result_content(&content, details, artifact_path);
                let _ = self
                    .transcript
                    .replace_tool_result_content(tool.tool_result_id, formatted);
                self.projection_dirty = true;
                self.dirty = true;
                RenderIntent::Render
            }
            FullscreenCommand::TurnEnd => {
                self.finish_active_turn("complete");
                RenderIntent::Render
            }
            FullscreenCommand::TurnAborted => {
                self.finish_active_turn("aborted");
                RenderIntent::Render
            }
            FullscreenCommand::TurnError { message } => {
                self.status_line = message;
                self.finish_active_turn("error");
                RenderIntent::Render
            }
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
            (KeyCode::Enter, KeyModifiers::NONE) | (KeyCode::Char(' '), KeyModifiers::NONE) => {
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
                    self.status_line =
                        "search scaffold ready • type after / to filter transcript".to_string();
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

        if matches!(
            self.mode,
            FullscreenMode::Transcript | FullscreenMode::Search
        ) && self.focused_block.is_none()
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

    fn current_layout(&self) -> FullscreenLayout {
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
            .unwrap_or_else(|| {
                if step.is_negative() {
                    blocks.len() - 1
                } else {
                    0
                }
            });

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
            self.status_line = format!(
                "focused {} block is not expandable",
                self.block_label(block_id)
            );
            self.dirty = true;
            return;
        }

        let next_collapsed = !block.collapsed;
        let action = if next_collapsed {
            "collapsed"
        } else {
            "expanded"
        };
        self.set_block_collapsed(block_id, next_collapsed, action);
    }

    fn set_block_collapsed(&mut self, block_id: BlockId, collapsed: bool, action: &str) {
        let Some(block) = self.transcript.block(block_id).cloned() else {
            return;
        };
        if !(block.expandable || !block.children.is_empty()) {
            self.status_line = format!(
                "focused {} block is not expandable",
                self.block_label(block_id)
            );
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

        self.projection_dirty = true;
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

    fn finish_active_turn(&mut self, status: &str) {
        if let Some(active_turn) = self.active_turn.take() {
            let _ = self.transcript.update_title(
                active_turn.root_id,
                format!("turn {} • {status}", active_turn.turn_index + 1),
            );
            self.projection_dirty = true;
            self.dirty = true;
        }
    }

    fn ensure_active_turn_root(&mut self) -> Option<BlockId> {
        self.active_turn.as_ref().map(|turn| turn.root_id)
    }

    fn ensure_thinking_block(&mut self) -> Result<BlockId, ()> {
        let Some(turn_root_id) = self.ensure_active_turn_root() else {
            return Err(());
        };
        if let Some(id) = self.active_turn.as_ref().and_then(|turn| turn.thinking_id) {
            return Ok(id);
        }
        let id = self
            .transcript
            .append_child_block(turn_root_id, NewBlock::new(BlockKind::Thinking, "thinking"))
            .map_err(|_| ())?;
        if let Some(active_turn) = self.active_turn.as_mut() {
            active_turn.thinking_id = Some(id);
        }
        Ok(id)
    }

    fn ensure_assistant_content_block(&mut self) -> Result<BlockId, ()> {
        let Some(turn_root_id) = self.ensure_active_turn_root() else {
            return Err(());
        };
        if let Some(id) = self.active_turn.as_ref().and_then(|turn| turn.content_id) {
            return Ok(id);
        }
        let id = self
            .transcript
            .append_child_block(
                turn_root_id,
                NewBlock::new(BlockKind::AssistantMessage, "response"),
            )
            .map_err(|_| ())?;
        if let Some(active_turn) = self.active_turn.as_mut() {
            active_turn.content_id = Some(id);
        }
        Ok(id)
    }

    fn tool_call_state(&self, id: &str) -> Option<&ToolCallState> {
        self.active_turn.as_ref()?.tools.get(id)
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
        self.pending_submissions.push_back(submitted);
        self.input.clear();
        self.cursor = 0;
        self.status_line = "submitted prompt • waiting for assistant".to_string();
        self.projection_dirty = true;
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
    let (_command_tx, command_rx) = mpsc::unbounded_channel();
    let (submission_tx, _submission_rx) = mpsc::unbounded_channel();
    run_with_channels(config, command_rx, submission_tx).await
}

pub async fn run_with_channels(
    config: FullscreenAppConfig,
    mut command_rx: mpsc::UnboundedReceiver<FullscreenCommand>,
    submission_tx: mpsc::UnboundedSender<String>,
) -> io::Result<FullscreenOutcome> {
    let mut terminal = FullscreenTerminal::enter()?;
    let (width, height) = terminal.size()?;
    let mut state = FullscreenState::new(config, Size { width, height });
    let mut renderer = FullscreenRenderer::new();
    let mut events = spawn_event_reader();
    let mut scheduler = RenderScheduler::default();
    let mut command_open = true;
    let mut tick = tokio::time::interval(Duration::from_millis(100));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    render_now(&mut terminal, &mut renderer, &mut state)?;
    flush_submissions(&mut state, &submission_tx);

    loop {
        if state.should_quit {
            break;
        }

        let scheduled_deadline = scheduler.next_flush_at();
        let scheduled_flush = async move {
            match scheduled_deadline {
                Some(deadline) => {
                    tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)).await;
                }
                None => std::future::pending::<()>().await,
            }
        };
        tokio::pin!(scheduled_flush);

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

                if state.dirty {
                    render_now(&mut terminal, &mut renderer, &mut state)?;
                    scheduler.clear();
                }
                flush_submissions(&mut state, &submission_tx);
            }
            maybe_command = command_rx.recv(), if command_open => {
                match maybe_command {
                    Some(command) => apply_render_intent(
                        state.apply_command(command),
                        &mut scheduler,
                        &mut terminal,
                        &mut renderer,
                        &mut state,
                    )?,
                    None => {
                        command_open = false;
                    }
                }
            }
            _ = &mut scheduled_flush => {
                if scheduler.should_flush(Instant::now()) {
                    render_now(&mut terminal, &mut renderer, &mut state)?;
                    scheduler.on_flushed();
                }
            }
            _ = tick.tick() => {
                state.on_tick();
            }
        }

        flush_submissions(&mut state, &submission_tx);
    }

    if scheduler.is_dirty() || state.dirty {
        render_now(&mut terminal, &mut renderer, &mut state)?;
        scheduler.on_flushed();
    }

    Ok(state.outcome())
}

fn apply_render_intent(
    intent: RenderIntent,
    scheduler: &mut RenderScheduler,
    terminal: &mut FullscreenTerminal,
    renderer: &mut FullscreenRenderer,
    state: &mut FullscreenState,
) -> io::Result<()> {
    match intent {
        RenderIntent::None => {}
        RenderIntent::Schedule => scheduler.mark_dirty(Instant::now()),
        RenderIntent::Render => {
            render_now(terminal, renderer, state)?;
            scheduler.on_flushed();
        }
    }
    Ok(())
}

fn render_now(
    terminal: &mut FullscreenTerminal,
    renderer: &mut FullscreenRenderer,
    state: &mut FullscreenState,
) -> io::Result<()> {
    if !state.take_dirty() {
        return Ok(());
    }
    state.prepare_for_render();
    let frame = build_frame(state);
    renderer.render(terminal, &frame)
}

fn flush_submissions(state: &mut FullscreenState, submission_tx: &mpsc::UnboundedSender<String>) {
    for submitted in state.take_pending_submissions() {
        if submission_tx.send(submitted).is_err() {
            break;
        }
    }
}

fn format_tool_result_content(
    content: &[ContentBlock],
    details: Option<serde_json::Value>,
    artifact_path: Option<String>,
) -> String {
    let mut lines = Vec::new();
    let mut text_lines = 0usize;
    let max_lines = 20usize;

    for block in content {
        match block {
            ContentBlock::Text { text } => {
                for line in text.lines() {
                    lines.push(line.to_string());
                    text_lines += 1;
                    if text_lines >= max_lines {
                        lines.push("… output truncated".to_string());
                        break;
                    }
                }
            }
            ContentBlock::Image { mime_type, .. } => {
                lines.push(format!("[image: {mime_type}]"));
                text_lines += 1;
            }
        }
        if text_lines >= max_lines {
            break;
        }
    }

    if let Some(details) = details {
        let details =
            serde_json::to_string_pretty(&details).unwrap_or_else(|_| details.to_string());
        if !details.trim().is_empty() {
            if !lines.is_empty() {
                lines.push(String::new());
            }
            lines.push("details:".to_string());
            lines.extend(details.lines().map(str::to_string));
        }
    }

    if let Some(path) = artifact_path {
        if !lines.is_empty() {
            lines.push(String::new());
        }
        lines.push(format!("artifact: {path}"));
    }

    if lines.is_empty() {
        "(no textual output)".to_string()
    } else {
        lines.join("\n")
    }
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

    #[test]
    fn streaming_updates_do_not_force_auto_follow_back_to_bottom() {
        let mut transcript = Transcript::new();
        for idx in 0..8 {
            transcript.append_root_block(
                NewBlock::new(BlockKind::SystemNote, format!("note {idx}"))
                    .with_content(format!("line {idx}")),
            );
        }

        let mut state = FullscreenState::new(
            FullscreenAppConfig {
                transcript,
                ..FullscreenAppConfig::default()
            },
            Size {
                width: 80,
                height: 12,
            },
        );
        state.viewport.jump_to_top();
        state.projection_dirty = true;
        state.prepare_for_render();
        let top_before = state.viewport.viewport_top;

        let _ = state.apply_command(FullscreenCommand::TurnStart { turn_index: 0 });
        let _ = state.apply_command(FullscreenCommand::TextDelta("hello".to_string()));
        state.prepare_for_render();

        assert!(!state.viewport.auto_follow);
        assert_eq!(state.viewport.viewport_top, top_before);
    }

    #[test]
    fn focused_transcript_anchor_is_preserved_during_streaming() {
        let mut transcript = Transcript::new();
        let first = transcript
            .append_root_block(NewBlock::new(BlockKind::SystemNote, "first").with_content("one"));
        transcript
            .append_root_block(NewBlock::new(BlockKind::SystemNote, "second").with_content("two"));
        transcript
            .append_root_block(NewBlock::new(BlockKind::SystemNote, "third").with_content("three"));

        let mut state = FullscreenState::new(
            FullscreenAppConfig {
                transcript,
                ..FullscreenAppConfig::default()
            },
            Size {
                width: 80,
                height: 12,
            },
        );
        state.on_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL));
        state.focused_block = Some(first);
        state.viewport.jump_to_top();
        state.viewport.auto_follow = false;
        state.sync_focus_tracking();
        let anchor_before = state
            .viewport
            .capture_header_anchor(&state.projection, first)
            .expect("anchor should exist");

        let _ = state.apply_command(FullscreenCommand::TurnStart { turn_index: 0 });
        let _ = state.apply_command(FullscreenCommand::TextDelta("delta".into()));
        state.prepare_for_render();

        let anchor_after = state
            .viewport
            .capture_header_anchor(&state.projection, first)
            .expect("anchor should still exist");
        assert_eq!(anchor_after.screen_offset, anchor_before.screen_offset);
        assert_eq!(state.focused_block, Some(first));
    }

    #[test]
    fn command_deltas_update_only_shared_transcript_blocks() {
        let mut state = FullscreenState::new(
            FullscreenAppConfig::default(),
            Size {
                width: 80,
                height: 24,
            },
        );

        let _ = state.apply_command(FullscreenCommand::TurnStart { turn_index: 0 });
        let _ = state.apply_command(FullscreenCommand::ThinkingDelta("thinking".into()));
        let _ = state.apply_command(FullscreenCommand::ToolCallStart {
            id: "tool-1".into(),
            name: "bash".into(),
        });
        let _ = state.apply_command(FullscreenCommand::ToolCallDelta {
            id: "tool-1".into(),
            args: "{\"command\":\"ls\"}".into(),
        });
        let _ = state.apply_command(FullscreenCommand::ToolResult {
            id: "tool-1".into(),
            name: "bash".into(),
            content: vec![ContentBlock::Text {
                text: "file.txt".into(),
            }],
            details: None,
            artifact_path: None,
            is_error: false,
        });
        let _ = state.apply_command(FullscreenCommand::TextDelta("done".into()));
        state.prepare_for_render();

        let assistant = state.transcript.root_blocks()[0];
        let assistant_block = state
            .transcript
            .block(assistant)
            .expect("assistant root should exist");
        assert_eq!(assistant_block.kind, BlockKind::AssistantMessage);
        assert_eq!(assistant_block.children.len(), 3);

        let thinking = state
            .transcript
            .block(assistant_block.children[0])
            .expect("thinking block should exist");
        assert_eq!(thinking.kind, BlockKind::Thinking);
        assert_eq!(thinking.content, "thinking");

        let tool_use = state
            .transcript
            .block(assistant_block.children[1])
            .expect("tool use block should exist");
        assert_eq!(tool_use.kind, BlockKind::ToolUse);
        assert!(tool_use.content.contains("ls"));

        let tool_result = state
            .transcript
            .block(tool_use.children[0])
            .expect("tool result block should exist");
        assert_eq!(tool_result.kind, BlockKind::ToolResult);
        assert!(tool_result.content.contains("file.txt"));

        let response = state
            .transcript
            .block(assistant_block.children[2])
            .expect("assistant response block should exist");
        assert_eq!(response.kind, BlockKind::AssistantMessage);
        assert_eq!(response.content, "done");
    }

    #[test]
    fn scheduler_batches_streaming_bursts_until_idle_or_frame_cap() {
        let start = Instant::now();
        let mut scheduler =
            RenderScheduler::new(Duration::from_millis(30), Duration::from_millis(10));

        scheduler.mark_dirty(start);
        scheduler.mark_dirty(start + Duration::from_millis(8));
        scheduler.mark_dirty(start + Duration::from_millis(16));

        assert!(!scheduler.should_flush(start + Duration::from_millis(24)));
        assert!(scheduler.should_flush(start + Duration::from_millis(26)));

        scheduler.on_flushed();
        scheduler.mark_dirty(start + Duration::from_millis(40));
        assert!(scheduler.should_flush(start + Duration::from_millis(70)));
    }
}
