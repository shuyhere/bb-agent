use std::collections::{HashMap, VecDeque};
use std::io;
use std::time::{Duration, Instant};

use bb_core::types::ContentBlock;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tokio::sync::mpsc;

use crate::select_list::{SelectItem, SelectList};
use crate::slash_commands::{
    matches_shared_local_slash_submission, shared_slash_command_select_items,
};

use super::{
    frame::{build_frame, measure_input},
    layout::{FullscreenLayout, Size, compute_layout_with_footer},
    projection::{TranscriptProjection, TranscriptProjector},
    renderer::FullscreenRenderer,
    scheduler::{RenderIntent, RenderScheduler},
    terminal::{FullscreenEvent, FullscreenTerminal, spawn_event_reader},
    transcript::{BlockId, BlockKind, NewBlock, Transcript},
    viewport::ViewportState,
};

#[derive(Clone, Debug, Default)]
pub struct FullscreenFooterData {
    pub line1: String,
    pub line2_left: String,
    pub line2_right: String,
}

#[derive(Clone, Debug)]
pub struct FullscreenAppConfig {
    pub title: String,
    pub input_placeholder: String,
    pub status_line: String,
    pub footer: FullscreenFooterData,
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
            footer: FullscreenFooterData::default(),
            transcript: Transcript::new(),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct FullscreenOutcome {
    pub submitted_inputs: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FullscreenSubmission {
    Input(String),
    MenuSelection { menu_id: String, value: String },
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
    SetFooter(FullscreenFooterData),
    SetTranscript(Transcript),
    SetInput(String),
    OpenSelectMenu {
        menu_id: String,
        title: String,
        items: Vec<SelectItem>,
    },
    CloseSelectMenu,
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
    raw_args: String,
    tool_use_id: BlockId,
    tool_result_id: Option<BlockId>,
    execution_started: bool,
    result_content: Option<Vec<ContentBlock>>,
    result_details: Option<serde_json::Value>,
    artifact_path: Option<String>,
    is_error: bool,
}

#[derive(Clone, Debug)]
pub(super) struct FullscreenSlashMenuState {
    all_items: Vec<SelectItem>,
    pub(super) list: SelectList,
}

#[derive(Clone, Debug)]
pub(super) struct FullscreenSelectMenuState {
    pub(super) menu_id: String,
    title: String,
    pub(super) list: SelectList,
}

fn colorize_tree_menu_label(label: &str) -> String {
    let t = crate::theme::theme();
    label
        .replace("[U]", &format!("{}[U]{}", t.cyan, t.reset))
        .replace("[A]", &format!("{}[A]{}", t.green, t.reset))
        .replace("[T]", &format!("{}[T]{}", t.yellow, t.reset))
        .replace("[C]", &format!("{}[C]{}", t.dim, t.reset))
        .replace("[B]", &format!("{}[B]{}", t.accent, t.reset))
        .replace("[?]", &format!("{}[?]{}", t.dim, t.reset))
}

impl FullscreenSelectMenuState {
    fn new(menu_id: String, title: String, mut items: Vec<SelectItem>) -> Self {
        if menu_id == "tree-entry" {
            for item in &mut items {
                item.label = colorize_tree_menu_label(&item.label);
            }
        }
        let mut list = SelectList::new(items, 8);
        list.set_show_search(false);
        Self { menu_id, title, list }
    }

    fn render(&self, width: usize) -> Vec<String> {
        let mut lines = vec![crate::utils::pad_to_width(
            &crate::utils::truncate_to_width(
                &format!("{} (Enter select, Esc close)", self.title),
                width,
            ),
            width,
        )];
        lines.extend(
            self.list
                .render(width as u16)
                .into_iter()
                .map(|line| crate::utils::pad_to_width(&crate::utils::truncate_to_width(&line, width), width)),
        );
        lines
    }

    fn rendered_height(&self) -> u16 {
        self.render(80).len() as u16
    }
}

impl FullscreenSlashMenuState {
    pub(super) fn new() -> Self {
        let all_items = shared_slash_command_select_items();
        let mut list = SelectList::new(all_items.clone(), 6);
        list.set_show_search(false);
        Self { all_items, list }
    }

    fn set_search(&mut self, query: &str) {
        let q = query.trim_start_matches('/').to_ascii_lowercase();
        let items = self
            .all_items
            .iter()
            .filter(|item| {
                if q.is_empty() {
                    true
                } else {
                    item.label
                        .trim_start_matches('/')
                        .to_ascii_lowercase()
                        .starts_with(&q)
                        || item
                            .value
                            .trim_start_matches('/')
                            .to_ascii_lowercase()
                            .starts_with(&q)
                }
            })
            .cloned()
            .collect::<Vec<_>>();
        let mut list = SelectList::new(items, 6);
        list.set_show_search(false);
        self.list = list;
    }

    pub(super) fn selected_value(&self) -> Option<String> {
        self.list.selected_value()
    }

    fn render(&self, width: usize) -> Vec<String> {
        let mut lines = self
            .list
            .render(width as u16)
            .into_iter()
            .map(|line| line.replace(" items", " commands"))
            .collect::<Vec<_>>();
        if lines.is_empty() {
            lines.push("  (no matching commands)".to_string());
        }
        lines
    }

    fn rendered_height(&self) -> u16 {
        self.render(80).len() as u16
    }
}

#[derive(Clone, Debug)]
pub struct FullscreenState {
    pub title: String,
    pub input_placeholder: String,
    pub status_line: String,
    pub transcript: Transcript,
    pub footer: FullscreenFooterData,
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
    pub(super) slash_menu: Option<FullscreenSlashMenuState>,
    pub(super) select_menu: Option<FullscreenSelectMenuState>,
    pub(super) projection_dirty: bool,
    pub(super) pending_submissions: VecDeque<FullscreenSubmission>,
    active_turn: Option<ActiveTurnState>,
    tool_output_expanded: bool,
}

impl FullscreenState {
    pub fn new(config: FullscreenAppConfig, size: Size) -> Self {
        let mut state = Self {
            title: config.title,
            input_placeholder: config.input_placeholder,
            status_line: config.status_line,
            transcript: config.transcript,
            footer: config.footer,
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
            slash_menu: None,
            select_menu: None,
            projection_dirty: true,
            pending_submissions: VecDeque::new(),
            active_turn: None,
            tool_output_expanded: false,
        };
        state.prepare_for_render();
        state
    }

    pub fn take_dirty(&mut self) -> bool {
        let dirty = self.dirty;
        self.dirty = false;
        dirty
    }

    pub fn take_pending_submissions(&mut self) -> Vec<FullscreenSubmission> {
        self.pending_submissions.drain(..).collect()
    }

    pub fn outcome(&self) -> FullscreenOutcome {
        FullscreenOutcome {
            submitted_inputs: self.submitted_inputs.clone(),
        }
    }

    pub(crate) fn has_active_turn(&self) -> bool {
        self.active_turn.is_some()
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
            FullscreenCommand::SetFooter(footer) => {
                self.footer = footer;
                self.dirty = true;
                RenderIntent::Render
            }
            FullscreenCommand::SetTranscript(transcript) => {
                self.transcript = transcript;
                self.active_turn = None;
                self.focused_block = None;
                self.search = FullscreenSearchState::default();
                self.mode = FullscreenMode::Normal;
                self.viewport.auto_follow = true;
                self.projection_dirty = true;
                self.dirty = true;
                RenderIntent::Render
            }
            FullscreenCommand::SetInput(input) => {
                self.input = input;
                self.cursor = self.input.len();
                self.slash_menu = None;
                self.select_menu = None;
                self.dirty = true;
                RenderIntent::Render
            }
            FullscreenCommand::OpenSelectMenu {
                menu_id,
                title,
                items,
            } => {
                self.slash_menu = None;
                self.select_menu = Some(FullscreenSelectMenuState::new(menu_id, title, items));
                self.dirty = true;
                RenderIntent::Render
            }
            FullscreenCommand::CloseSelectMenu => {
                self.select_menu = None;
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
                if let Some(active_turn) = self.active_turn.as_mut() {
                    active_turn.tools.insert(
                        id.clone(),
                        ToolCallState {
                            name,
                            raw_args: String::new(),
                            tool_use_id,
                            tool_result_id: None,
                            execution_started: false,
                            result_content: None,
                            result_details: None,
                            artifact_path: None,
                            is_error: false,
                        },
                    );
                }
                self.refresh_tool_rendering(&id);
                RenderIntent::Render
            }
            FullscreenCommand::ToolCallDelta { id, args } => {
                if args.is_empty() {
                    return RenderIntent::None;
                }
                match self.tool_call_state_mut(&id) {
                    Some(tool) => tool.raw_args.push_str(&args),
                    None => return RenderIntent::None,
                };
                self.refresh_tool_rendering(&id);
                RenderIntent::Schedule
            }
            FullscreenCommand::ToolExecuting { id } => {
                let Some(tool) = self.tool_call_state_mut(&id) else {
                    return RenderIntent::None;
                };
                tool.execution_started = true;
                self.refresh_tool_rendering(&id);
                RenderIntent::Render
            }
            FullscreenCommand::ToolResult {
                id,
                name: _,
                content,
                details,
                artifact_path,
                is_error,
            } => {
                let Some(tool) = self.tool_call_state_mut(&id) else {
                    return RenderIntent::None;
                };
                tool.result_content = Some(content);
                tool.result_details = details;
                tool.artifact_path = artifact_path;
                tool.is_error = is_error;
                self.refresh_tool_rendering(&id);
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



    pub(super) fn on_search_key(&mut self, key: KeyEvent) {
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

    pub(super) fn mode_help_text(&self) -> String {
        match self.mode {
            FullscreenMode::Normal => String::new(),
            FullscreenMode::Transcript => {
                "transcript mode • j/k navigate • Enter/Space toggle • o expand • c collapse • Ctrl+O tool output • / search • Esc returns"
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

    pub(super) fn search_prompt(&self) -> String {
        if self.search.query.is_empty() {
            "/".to_string()
        } else {
            format!("/{}", self.search.query)
        }
    }

    pub(crate) fn current_layout(&self) -> FullscreenLayout {
        let input_inner_width = self.size.width.max(1) as usize;
        let input_wrap = measure_input(&self.input, self.cursor, input_inner_width);
        compute_layout_with_footer(self.size, input_wrap.lines.len(), self.requested_footer_height())
    }

    pub(crate) fn requested_footer_height(&self) -> u16 {
        if let Some(menu) = self.select_menu.as_ref() {
            menu.rendered_height()
        } else if let Some(menu) = self.slash_menu.as_ref() {
            menu.rendered_height()
        } else if self.size.height >= 14 {
            2
        } else {
            0
        }
    }

    pub(crate) fn render_select_menu_lines(&self, width: usize) -> Option<Vec<String>> {
        self.select_menu.as_ref().map(|menu| menu.render(width))
    }

    pub(crate) fn render_slash_menu_lines(&self, width: usize) -> Option<Vec<String>> {
        self.slash_menu.as_ref().map(|menu| menu.render(width))
    }

    pub(super) fn slash_query(&self) -> Option<String> {
        let before = self.input.get(..self.cursor)?;
        if before.contains('\n') {
            return None;
        }
        if !before.starts_with('/') {
            return None;
        }
        if before.contains(' ') {
            return None;
        }
        Some(before.to_string())
    }

    pub(super) fn update_slash_menu(&mut self) {
        let Some(query) = self.slash_query() else {
            self.slash_menu = None;
            return;
        };
        let mut menu = self.slash_menu.take().unwrap_or_else(FullscreenSlashMenuState::new);
        menu.set_search(&query);
        self.slash_menu = Some(menu);
    }

    pub(super) fn accept_slash_selection(&mut self, value: String) {
        self.input = value;
        self.cursor = self.input.len();
        self.slash_menu = None;
        self.dirty = true;
    }

    pub(super) fn sync_focus_tracking(&mut self) {
        self.viewport.selected_block = self.focused_block;
        self.viewport.focused_row = self
            .focused_block
            .and_then(|block_id| self.focus_row_for_block(block_id));
    }

    pub(super) fn focus_first_visible_block(&mut self) {
        let visible = self.visible_header_blocks();
        if let Some(block_id) = visible.first().copied() {
            self.set_focused_block(Some(block_id));
        }
    }

    pub(super) fn focus_last_visible_block(&mut self) {
        let visible = self.visible_header_blocks();
        if let Some(block_id) = visible.last().copied() {
            self.set_focused_block(Some(block_id));
        }
    }

    pub(super) fn default_focus_block(&self) -> Option<BlockId> {
        self.visible_header_blocks()
            .last()
            .copied()
            .or_else(|| self.last_focusable_block())
            .or_else(|| self.first_focusable_block())
    }

    fn visible_header_blocks(&self) -> Vec<BlockId> {
        let mut visible = Vec::new();
        for row_index in self.viewport.visible_row_range() {
            let Some(row) = self.projection.row(row_index) else {
                continue;
            };
            if visible.last().copied() == Some(row.block_id) {
                continue;
            }
            if !visible.contains(&row.block_id) {
                visible.push(row.block_id);
            }
        }
        visible
    }

    fn focusable_blocks(&self) -> Vec<BlockId> {
        self.projection
            .rows
            .iter()
            .filter(|row| self.focus_row_for_block(row.block_id) == Some(row.index))
            .map(|row| row.block_id)
            .collect()
    }

    fn first_focusable_block(&self) -> Option<BlockId> {
        self.focusable_blocks().into_iter().next()
    }

    pub(super) fn last_focusable_block(&self) -> Option<BlockId> {
        self.focusable_blocks().into_iter().last()
    }

    pub(super) fn set_focused_block(&mut self, block_id: Option<BlockId>) {
        self.focused_block = block_id;
        self.sync_focus_tracking();
        self.dirty = true;
    }

    pub(super) fn focus_block(&mut self, block_id: BlockId) {
        self.focused_block = Some(block_id);
        self.viewport.auto_follow = false;
        self.sync_focus_tracking();
        self.ensure_focus_visible();
        self.dirty = true;
    }

    pub(super) fn focus_first(&mut self) {
        if let Some(block_id) = self.first_focusable_block() {
            self.focus_block(block_id);
        }
    }

    pub(super) fn focus_last(&mut self) {
        if let Some(block_id) = self.last_focusable_block() {
            self.focus_block(block_id);
        }
    }

    pub(super) fn move_focus(&mut self, step: isize) {
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

    pub(super) fn page_move(&mut self, direction: isize) {
        let step = self.visible_header_blocks().len().max(1);
        for _ in 0..step {
            self.move_focus(direction.signum());
        }
    }

    pub(super) fn ensure_focus_visible(&mut self) {
        let Some(block_id) = self.focused_block else {
            self.sync_focus_tracking();
            return;
        };
        let Some(focus_row) = self.focus_row_for_block(block_id) else {
            self.sync_focus_tracking();
            return;
        };

        if self.viewport.viewport_height == 0 {
            self.sync_focus_tracking();
            return;
        }

        if focus_row < self.viewport.viewport_top {
            self.viewport.viewport_top = focus_row;
            self.viewport.auto_follow = false;
        } else if focus_row >= self.viewport.viewport_top + self.viewport.viewport_height {
            self.viewport.viewport_top = focus_row
                .saturating_add(1)
                .saturating_sub(self.viewport.viewport_height);
            self.viewport.auto_follow = false;
        }
        self.sync_focus_tracking();
    }

    fn focus_row_for_block(&self, block_id: BlockId) -> Option<usize> {
        self.projection
            .header_row_for_block(block_id)
            .or_else(|| self.projection.rows_for_block(block_id).map(|span| span.all_rows.start))
    }

    pub(super) fn search_step(&mut self, forward: bool) {
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


    fn finish_active_turn(&mut self, status: &str) {
        if let Some(active_turn) = self.active_turn.take() {
            let _ = self.transcript.update_title(
                active_turn.root_id,
                format!("turn {} • {status}", active_turn.turn_index + 1),
            );
            self.projection_dirty = true;
            self.dirty = true;
        }
        // If still auto-following (user didn't manually scroll away), return
        // to Normal mode so the input area is focused and the user can type
        // immediately.  If the user deliberately entered Transcript mode and
        // scrolled away, stay there so they can keep reading.
        if self.viewport.auto_follow {
            self.mode = FullscreenMode::Normal;
        }
        // Clear the stale "Working..." status.
        self.status_line = self.mode_help_text();
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

    fn tool_call_state_mut(&mut self, id: &str) -> Option<&mut ToolCallState> {
        self.active_turn.as_mut()?.tools.get_mut(id)
    }

    fn ensure_tool_result_block(&mut self, id: &str) -> Option<BlockId> {
        let existing = self.tool_call_state(id)?.tool_result_id;
        if existing.is_some() {
            return existing;
        }

        let tool_use_id = self.tool_call_state(id)?.tool_use_id;
        let tool_result_id = self
            .transcript
            .append_child_block(tool_use_id, NewBlock::new(BlockKind::ToolResult, "output"))
            .ok()?;
        if let Some(tool) = self.tool_call_state_mut(id) {
            tool.tool_result_id = Some(tool_result_id);
        }
        Some(tool_result_id)
    }

    fn refresh_tool_rendering(&mut self, id: &str) {
        let Some(tool) = self.tool_call_state(id).cloned() else {
            return;
        };

        let display_name = format_tool_call_title(&tool.name, &tool.raw_args);
        let status = if tool.result_content.is_some() {
            if tool.is_error { "error" } else { "done" }
        } else if tool.execution_started {
            "running"
        } else {
            "collecting"
        };
        let _ = self
            .transcript
            .update_title(tool.tool_use_id, format!("{display_name} • {status}"));
        let _ = self.transcript.replace_content(
            tool.tool_use_id,
            format_tool_call_content(&tool.name, &tool.raw_args, self.tool_output_expanded),
        );

        if let Some(result_content) = tool.result_content.clone() {
            let Some(tool_result_id) = self.ensure_tool_result_block(id) else {
                return;
            };
            let _ = self.transcript.update_title(
                tool_result_id,
                if tool.is_error { "error output" } else { "output" },
            );
            let formatted = format_tool_result_content(
                &tool.name,
                &result_content,
                tool.result_details.clone(),
                tool.artifact_path.clone(),
                tool.is_error,
                self.tool_output_expanded,
            );
            let _ = self
                .transcript
                .replace_tool_result_content(tool_result_id, formatted);
        } else if tool.execution_started {
            let Some(tool_result_id) = self.ensure_tool_result_block(id) else {
                return;
            };
            let _ = self.transcript.update_title(tool_result_id, "output");
            let _ = self
                .transcript
                .replace_tool_result_content(tool_result_id, "executing...".to_string());
        }

        self.projection_dirty = true;
        self.dirty = true;
    }

    pub(super) fn toggle_tool_output_expansion(&mut self) {
        self.tool_output_expanded = !self.tool_output_expanded;
        if let Some(active_turn) = self.active_turn.as_ref() {
            let ids = active_turn.tools.keys().cloned().collect::<Vec<_>>();
            for id in ids {
                self.refresh_tool_rendering(&id);
            }
        }
        self.status_line = format!(
            "tool output expansion {}",
            if self.tool_output_expanded { "enabled" } else { "collapsed" }
        );
        self.projection_dirty = true;
        self.dirty = true;
    }

    pub(super) fn submit_input(&mut self) {
        let submitted = self.input.trim_end().to_string();
        if submitted.trim().is_empty() {
            self.status_line = "empty input ignored".to_string();
            self.dirty = true;
            return;
        }

        if matches_shared_local_slash_submission(&submitted) {
            self.submit_local_command(submitted);
            return;
        }

        self.transcript.append_root_block(
            NewBlock::new(BlockKind::UserMessage, "prompt").with_content(submitted.clone()),
        );
        self.submitted_inputs.push(submitted.clone());
        self.pending_submissions
            .push_back(FullscreenSubmission::Input(submitted));
        self.input.clear();
        self.cursor = 0;
        self.slash_menu = None;
        self.select_menu = None;
        self.status_line = "Working...".to_string();
        self.projection_dirty = true;
        self.dirty = true;
    }

    pub(super) fn submit_local_command(&mut self, submitted: String) {
        self.pending_submissions
            .push_back(FullscreenSubmission::Input(submitted));
        self.input.clear();
        self.cursor = 0;
        self.slash_menu = None;
        self.select_menu = None;
        self.status_line = self.mode_help_text();
        self.dirty = true;
    }

    pub(super) fn insert_char(&mut self, ch: char) {
        self.input.insert(self.cursor, ch);
        self.cursor += ch.len_utf8();
        self.update_slash_menu();
        self.dirty = true;
    }

    pub(super) fn insert_str(&mut self, text: &str) {
        self.input.insert_str(self.cursor, text);
        self.cursor += text.len();
        self.update_slash_menu();
        self.dirty = true;
    }

    pub(super) fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let prev = previous_boundary(&self.input, self.cursor);
        self.input.drain(prev..self.cursor);
        self.cursor = prev;
        self.update_slash_menu();
        self.dirty = true;
    }

    pub(super) fn move_left(&mut self) {
        if self.cursor == 0 {
            return;
        }
        self.cursor = previous_boundary(&self.input, self.cursor);
        self.update_slash_menu();
        self.dirty = true;
    }

    pub(super) fn move_right(&mut self) {
        if self.cursor >= self.input.len() {
            return;
        }
        self.cursor = next_boundary(&self.input, self.cursor);
        self.update_slash_menu();
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
    submission_tx: mpsc::UnboundedSender<FullscreenSubmission>,
) -> io::Result<FullscreenOutcome> {
    let mut terminal = FullscreenTerminal::enter()?;
    let (width, height) = terminal.size()?;
    let mut state = FullscreenState::new(config, Size { width, height });
    let mut renderer = FullscreenRenderer::new();
    let mut events = spawn_event_reader();
    let mut scheduler = RenderScheduler::default();
    let mut command_open = true;
    let mut tick = tokio::time::interval(Duration::from_millis(80));
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
                if state.dirty {
                    render_now(&mut terminal, &mut renderer, &mut state)?;
                    scheduler.clear();
                }
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

fn flush_submissions(
    state: &mut FullscreenState,
    submission_tx: &mpsc::UnboundedSender<FullscreenSubmission>,
) {
    for submitted in state.take_pending_submissions() {
        if submission_tx.send(submitted).is_err() {
            break;
        }
    }
}

fn format_tool_call_title(name: &str, raw_args: &str) -> String {
    let Ok(args) = serde_json::from_str::<serde_json::Value>(raw_args) else {
        return name.to_string();
    };

    match name {
        "bash" => {
            let first_line = args
                .get("command")
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .lines()
                .find(|line| !line.trim().is_empty())
                .unwrap_or_default()
                .trim()
                .to_string();
            if first_line.is_empty() {
                "bash".to_string()
            } else {
                format!("$ {first_line}")
            }
        }
        "read" => {
            let path = args.get("path").and_then(|value| value.as_str()).unwrap_or_default();
            let offset = args.get("offset").and_then(|value| value.as_u64());
            let limit = args.get("limit").and_then(|value| value.as_u64());
            let mut line_suffix = String::new();
            if offset.is_some() || limit.is_some() {
                let start = offset.unwrap_or(1);
                if let Some(limit) = limit {
                    let end = start.saturating_add(limit).saturating_sub(1);
                    line_suffix = format!(":{start}-{end}");
                } else {
                    line_suffix = format!(":{start}");
                }
            }
            if path.is_empty() {
                "read".to_string()
            } else {
                format!("read {}{line_suffix}", shorten_display_path(path))
            }
        }
        "write" | "edit" => {
            let path = args.get("path").and_then(|value| value.as_str()).unwrap_or_default();
            if path.is_empty() {
                name.to_string()
            } else {
                format!("{name} {}", shorten_display_path(path))
            }
        }
        "ls" => {
            let path = args.get("path").and_then(|value| value.as_str()).unwrap_or(".");
            let limit = args.get("limit").and_then(|value| value.as_u64());
            match limit {
                Some(limit) => format!("ls {} (limit {limit})", shorten_display_path(path)),
                None => format!("ls {}", shorten_display_path(path)),
            }
        }
        "grep" => {
            let pattern = args
                .get("pattern")
                .and_then(|value| value.as_str())
                .unwrap_or_default();
            let path = args.get("path").and_then(|value| value.as_str()).unwrap_or(".");
            let glob = args.get("glob").and_then(|value| value.as_str());
            let mut title = if pattern.is_empty() {
                format!("grep {}", shorten_display_path(path))
            } else {
                format!("grep /{pattern}/ in {}", shorten_display_path(path))
            };
            if let Some(glob) = glob.filter(|glob| !glob.is_empty()) {
                title.push_str(&format!(" ({glob})"));
            }
            title
        }
        "find" => {
            let pattern = args
                .get("pattern")
                .and_then(|value| value.as_str())
                .unwrap_or_default();
            let path = args.get("path").and_then(|value| value.as_str()).unwrap_or(".");
            if pattern.is_empty() {
                format!("find {}", shorten_display_path(path))
            } else {
                format!("find {pattern} in {}", shorten_display_path(path))
            }
        }
        _ => name.to_string(),
    }
}

fn shorten_display_path(path: &str) -> String {
    if path.is_empty() {
        return String::new();
    }
    if let Ok(home) = std::env::var("HOME") {
        if path.starts_with(&home) {
            return format!("~{}", &path[home.len()..]);
        }
    }
    path.to_string()
}

fn format_tool_call_content(name: &str, raw_args: &str, expanded: bool) -> String {
    crate::tool_preview::format_tool_call_content(name, raw_args, expanded)
}

fn format_tool_result_content(
    name: &str,
    content: &[ContentBlock],
    details: Option<serde_json::Value>,
    artifact_path: Option<String>,
    is_error: bool,
    expanded: bool,
) -> String {
    crate::tool_preview::format_tool_result_content(
        name,
        content,
        details,
        artifact_path,
        is_error,
        expanded,
    )
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
    use crate::fullscreen::frame::build_frame;
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

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
                height: 16,
            },
        );
        (state, intro, tool, result)
    }

    fn scrolling_state() -> (FullscreenState, Vec<BlockId>) {
        let mut transcript = Transcript::new();
        let mut blocks = Vec::new();
        for idx in 0..10 {
            let block_id = transcript.append_root_block(
                NewBlock::new(BlockKind::AssistantMessage, format!("message {idx}"))
                    .with_content(format!("line {idx}\nmore detail {idx}")),
            );
            blocks.push(block_id);
        }

        let state = FullscreenState::new(
            FullscreenAppConfig {
                transcript,
                ..FullscreenAppConfig::default()
            },
            Size {
                width: 60,
                height: 10,
            },
        );
        (state, blocks)
    }

    fn screen_row_for_header(state: &FullscreenState, block_id: BlockId) -> u16 {
        let header_row = state
            .projection
            .header_row_for_block(block_id)
            .expect("header row should exist");
        let local_row = header_row.saturating_sub(state.viewport.viewport_top);
        let layout = state.current_layout();
        layout.transcript.y + local_row as u16
    }

    #[test]
    fn frame_renders_header_title_when_space_allows() {
        let mut state = FullscreenState::new(
            FullscreenAppConfig {
                title: "BB-Agent v0.1.0".to_string(),
                ..FullscreenAppConfig::default()
            },
            Size {
                width: 80,
                height: 12,
            },
        );
        state.prepare_for_render();
        let frame = build_frame(&state);

        assert!(frame.lines[0].contains("BB-Agent v0.1.0"));
        assert!(frame.lines[1].contains("Ctrl-C exit"));
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
        let screen_row = screen_row_for_header(&state, tool);

        state.on_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 0,
            row: screen_row,
            modifiers: KeyModifiers::NONE,
        });

        assert_eq!(state.mode, FullscreenMode::Transcript);
        assert_eq!(state.focused_block, Some(tool));
        assert!(!state.viewport.auto_follow);
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

        let assistant = state.transcript.root_blocks()[0];
        let assistant_block = state
            .transcript
            .block(assistant)
            .expect("assistant root should exist");
        let tool_use_before_result = state
            .transcript
            .block(assistant_block.children[1])
            .expect("tool use block should exist before result");
        assert!(tool_use_before_result.children.is_empty());

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
        assert!(tool_use.title.contains("ls"));

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
    fn tool_executing_shows_placeholder_and_ctrl_o_expands_output() {
        let mut state = FullscreenState::new(
            FullscreenAppConfig::default(),
            Size {
                width: 80,
                height: 24,
            },
        );

        let _ = state.apply_command(FullscreenCommand::TurnStart { turn_index: 0 });
        let _ = state.apply_command(FullscreenCommand::ToolCallStart {
            id: "tool-1".into(),
            name: "bash".into(),
        });
        let _ = state.apply_command(FullscreenCommand::ToolCallDelta {
            id: "tool-1".into(),
            args: serde_json::json!({ "command": "printf 'a\\nb\\nc\\nd\\ne\\nf\\ng\\nh\\ni\\nj\\nk\\nl\\nm\\nn'" }).to_string(),
        });
        let _ = state.apply_command(FullscreenCommand::ToolExecuting {
            id: "tool-1".into(),
        });

        let assistant = state.transcript.root_blocks()[0];
        let assistant_block = state.transcript.block(assistant).expect("assistant root");
        let tool_use_id = assistant_block.children[0];
        let tool_use = state.transcript.block(tool_use_id).expect("tool use");
        let tool_result = state
            .transcript
            .block(tool_use.children[0])
            .expect("tool result placeholder");
        assert!(tool_result.content.contains("executing..."));

        let _ = state.apply_command(FullscreenCommand::ToolResult {
            id: "tool-1".into(),
            name: "bash".into(),
            content: vec![ContentBlock::Text {
                text: (1..=14).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n"),
            }],
            details: None,
            artifact_path: None,
            is_error: false,
        });
        let tool_use = state.transcript.block(tool_use_id).expect("tool use after result");
        let tool_result = state
            .transcript
            .block(tool_use.children[0])
            .expect("tool result after result");
        assert!(tool_result.content.contains("... (2 more lines; Ctrl+O to expand)"));
        assert!(!tool_result.content.contains("line 14"));

        state.mode = FullscreenMode::Transcript;
        state.focused_block = Some(tool_use_id);
        state.on_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL));

        let tool_use = state.transcript.block(tool_use_id).expect("tool use after expand");
        let tool_result = state
            .transcript
            .block(tool_use.children[0])
            .expect("tool result after expand");
        assert!(!tool_result.content.contains("... (2 more lines; Ctrl+O to expand)"));
        assert!(tool_result.content.contains("line 14"));
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

    #[test]
    fn scroll_events_toggle_follow_and_focus_the_visible_edge() {
        let (mut state, _) = scrolling_state();
        let transcript_row = state.current_layout().transcript.y;

        state.on_mouse(MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 0,
            row: transcript_row,
            modifiers: KeyModifiers::NONE,
        });

        assert_eq!(state.mode, FullscreenMode::Transcript);
        assert!(!state.viewport.auto_follow);
        assert_eq!(
            state.focused_block,
            state.visible_header_blocks().first().copied()
        );
        assert!(state.status_line.contains("follow off"));

        for _ in 0..10 {
            state.on_mouse(MouseEvent {
                kind: MouseEventKind::ScrollDown,
                column: 0,
                row: transcript_row,
                modifiers: KeyModifiers::NONE,
            });
            if state.viewport.auto_follow {
                break;
            }
        }

        assert!(state.viewport.auto_follow);
        assert_eq!(
            state.focused_block,
            state.visible_header_blocks().last().copied()
        );
        assert!(state.status_line.contains("follow on"));
    }

    #[test]
    fn ctrl_j_submits_like_enter_in_normal_mode() {
        let mut state = FullscreenState::new(
            FullscreenAppConfig::default(),
            Size {
                width: 80,
                height: 24,
            },
        );
        state.input = "hello".to_string();
        state.cursor = state.input.len();

        state.on_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL));

        assert!(state.input.is_empty());
        assert_eq!(state.submitted_inputs, vec!["hello".to_string()]);
        assert_eq!(state.status_line, "Working...");
    }

    #[test]
    fn edit_tool_result_prefers_diff_when_available() {
        let rendered = format_tool_result_content(
            "edit",
            &[],
            Some(serde_json::json!({
                "path": "/tmp/demo.txt",
                "applied": 1,
                "total": 1,
                "diff": "@@ -1 +1 @@\n-old\n+new"
            })),
            None,
            false,
            false,
        );

        assert!(rendered.contains("applied 1/1 edit(s) to /tmp/demo.txt"));
        assert!(rendered.contains("@@ -1 +1 @@"));
        assert!(rendered.contains("-old"));
        assert!(rendered.contains("+new"));
    }

    #[test]
    fn tool_titles_and_results_shorten_home_paths() {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/home/test".to_string());
        let path = format!("{home}/project/demo.txt");
        let raw_args = serde_json::json!({ "path": path }).to_string();

        let title = format_tool_call_title("read", &raw_args);
        assert!(title.contains("~/project/demo.txt") || title.contains("/project/demo.txt"));

        let rendered = format_tool_result_content(
            "write",
            &[],
            Some(serde_json::json!({
                "path": format!("{home}/project/demo.txt"),
                "bytes": 12
            })),
            None,
            false,
            false,
        );
        assert!(rendered.contains("wrote 12 bytes to ~/project/demo.txt") || rendered.contains("wrote 12 bytes to /home/test/project/demo.txt"));
    }

    #[test]
    fn tool_titles_include_interactive_context_details() {
        let read = format_tool_call_title(
            "read",
            &serde_json::json!({
                "path": "/tmp/demo.txt",
                "offset": 5,
                "limit": 3
            })
            .to_string(),
        );
        assert_eq!(read, "read /tmp/demo.txt:5-7");

        let ls = format_tool_call_title(
            "ls",
            &serde_json::json!({
                "path": "/tmp",
                "limit": 25
            })
            .to_string(),
        );
        assert_eq!(ls, "ls /tmp (limit 25)");

        let grep = format_tool_call_title(
            "grep",
            &serde_json::json!({
                "pattern": "todo",
                "path": "/tmp/project",
                "glob": "*.rs"
            })
            .to_string(),
        );
        assert_eq!(grep, "grep /todo/ in /tmp/project (*.rs)");

        let find = format_tool_call_title(
            "find",
            &serde_json::json!({
                "pattern": "*.md",
                "path": "/tmp/project"
            })
            .to_string(),
        );
        assert_eq!(find, "find *.md in /tmp/project");
    }

    #[test]
    fn artifact_paths_shorten_home_prefix() {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/home/test".to_string());
        let rendered = format_tool_result_content(
            "write",
            &[],
            None,
            Some(format!("{home}/project/out.patch")),
            false,
            false,
        );
        assert!(
            rendered.contains("artifact: ~/project/out.patch")
                || rendered.contains("artifact: /home/test/project/out.patch")
        );
    }

    #[test]
    fn write_and_edit_call_content_use_interactive_style_previews() {
        let write = format_tool_call_content(
            "write",
            &serde_json::json!({
                "path": "/tmp/demo.txt",
                "content": "one\ntwo\nthree\nfour\nfive\nsix"
            })
            .to_string(),
            false,
        );
        assert!(write.contains("one"));
        assert!(write.contains("five"));
        assert!(write.contains("... (1 more lines; Ctrl+O to expand)"));
        assert!(!write.contains("\"content\""));

        let edit = format_tool_call_content(
            "edit",
            &serde_json::json!({
                "path": "/tmp/demo.txt",
                "edits": [
                    { "oldText": "alpha", "newText": "beta" },
                    { "oldText": "line1\nline2", "newText": "line1\nlineX" }
                ]
            })
            .to_string(),
            false,
        );
        assert!(edit.contains("2 edit block(s)"));
        assert!(edit.contains("1. - alpha"));
        assert!(edit.contains("+ beta"));
        assert!(edit.contains("line1\\nline2"));
        assert!(!edit.contains("\"oldText\""));
    }

    #[test]
    fn tool_result_previews_use_interactive_limits_and_truncation() {
        let bash_lines = (1..=14)
            .map(|i| format!("line\t{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let bash = format_tool_result_content(
            "bash",
            &[ContentBlock::Text { text: bash_lines.clone() }],
            None,
            None,
            false,
            false,
        );
        assert!(bash.contains("line   1"));
        assert!(bash.contains("line   12"));
        assert!(bash.contains("... (2 more lines; Ctrl+O to expand)"));
        assert!(!bash.contains("… output truncated"));
        assert!(!bash.contains("line   13\nline   14"));

        let grep_lines = (1..=16)
            .map(|i| format!("match {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let grep = format_tool_result_content(
            "grep",
            &[ContentBlock::Text { text: grep_lines.clone() }],
            None,
            None,
            false,
            false,
        );
        assert!(grep.contains("match 15"));
        assert!(grep.contains("... (1 more lines; Ctrl+O to expand)"));

        let expanded = format_tool_result_content(
            "bash",
            &[ContentBlock::Text { text: bash_lines }],
            None,
            None,
            false,
            true,
        );
        assert!(expanded.contains("line   14"));
        assert!(!expanded.contains("... (2 more lines; Ctrl+O to expand)"));
    }

    #[test]
    fn typing_slash_in_normal_mode_shows_fullscreen_command_menu() {
        let mut state = FullscreenState::new(
            FullscreenAppConfig::default(),
            Size {
                width: 80,
                height: 20,
            },
        );

        state.on_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        let lines = state
            .render_slash_menu_lines(80)
            .expect("slash menu should be visible");
        let joined = lines.join("\n");
        assert!(joined.contains("/model"));
        assert!(joined.contains("/copy"));
        assert!(state.requested_footer_height() >= 6);

        state.on_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        let lines = state
            .render_slash_menu_lines(80)
            .expect("filtered slash menu should be visible");
        let joined = lines.join("\n");
        assert!(joined.contains("/settings"));
    }

    #[test]
    fn slash_menu_scrolls_when_selection_moves_past_visible_window() {
        let mut state = FullscreenState::new(
            FullscreenAppConfig::default(),
            Size {
                width: 80,
                height: 20,
            },
        );

        state.on_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        for _ in 0..6 {
            state.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        }

        assert!(state.slash_menu.is_some());

        let joined = state
            .render_slash_menu_lines(80)
            .expect("slash menu should render")
            .join("\n");
        assert!(joined.contains("more above"));
        assert!(joined.contains("/tree") || joined.contains("/fork") || joined.contains("/new"));
    }

    #[test]
    fn enter_on_hidden_scrolled_slash_item_accepts_that_item() {
        let mut state = FullscreenState::new(
            FullscreenAppConfig::default(),
            Size {
                width: 80,
                height: 20,
            },
        );

        state.on_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        for _ in 0..6 {
            state.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        }
        let expected = state
            .slash_menu
            .as_ref()
            .and_then(|menu| menu.selected_value())
            .expect("selected slash command");

        state.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert_eq!(state.input, expected);
        assert!(state.slash_menu.is_none());
    }

    #[test]
    fn tab_accepts_slash_menu_selection() {
        let mut state = FullscreenState::new(
            FullscreenAppConfig::default(),
            Size {
                width: 80,
                height: 20,
            },
        );

        state.on_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        state.on_key(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE));
        state.on_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));

        assert_eq!(state.input, "/model");
        assert!(state.slash_menu.is_none());
    }

    #[test]
    fn enter_submits_exact_slash_command_without_waiting_for_second_enter() {
        let mut state = FullscreenState::new(
            FullscreenAppConfig::default(),
            Size {
                width: 80,
                height: 20,
            },
        );

        for ch in "/model".chars() {
            state.on_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        state.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert!(state.input.is_empty());
        assert!(state.transcript.root_blocks().is_empty());
        assert!(state.status_line.is_empty());
        assert_eq!(
            state.take_pending_submissions(),
            vec![FullscreenSubmission::Input("/model".to_string())]
        );
    }

    #[test]
    fn ctrl_j_submits_exact_slash_command_without_llm_send_path() {
        let mut state = FullscreenState::new(
            FullscreenAppConfig::default(),
            Size {
                width: 80,
                height: 20,
            },
        );

        for ch in "/settings".chars() {
            state.on_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        state.on_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL));

        assert!(state.input.is_empty());
        assert!(state.transcript.root_blocks().is_empty());
        assert!(state.status_line.is_empty());
        assert_eq!(
            state.take_pending_submissions(),
            vec![FullscreenSubmission::Input("/settings".to_string())]
        );
    }

    #[test]
    fn enter_submits_argument_slash_command_without_prompt_echo_or_working() {
        let mut state = FullscreenState::new(
            FullscreenAppConfig::default(),
            Size {
                width: 80,
                height: 20,
            },
        );

        for ch in "/name demo".chars() {
            state.on_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        state.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert!(state.input.is_empty());
        assert!(state.transcript.root_blocks().is_empty());
        assert!(state.status_line.is_empty());
        assert_eq!(
            state.take_pending_submissions(),
            vec![FullscreenSubmission::Input("/name demo".to_string())]
        );
    }

    #[test]
    fn select_menu_enter_emits_control_submission() {
        let mut state = FullscreenState::new(
            FullscreenAppConfig::default(),
            Size {
                width: 80,
                height: 20,
            },
        );

        let _ = state.apply_command(FullscreenCommand::OpenSelectMenu {
            menu_id: "model".to_string(),
            title: "Select model".to_string(),
            items: vec![
                SelectItem {
                    label: "anthropic/claude".to_string(),
                    detail: None,
                    value: "anthropic/claude".to_string(),
                },
                SelectItem {
                    label: "openai/gpt-4o".to_string(),
                    detail: None,
                    value: "openai/gpt-4o".to_string(),
                },
            ],
        });
        state.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        state.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert_eq!(
            state.take_pending_submissions(),
            vec![FullscreenSubmission::MenuSelection {
                menu_id: "model".to_string(),
                value: "openai/gpt-4o".to_string(),
            }]
        );
    }

    #[test]
    fn select_menu_ctrl_j_emits_control_submission() {
        let mut state = FullscreenState::new(
            FullscreenAppConfig::default(),
            Size {
                width: 80,
                height: 20,
            },
        );

        let _ = state.apply_command(FullscreenCommand::OpenSelectMenu {
            menu_id: "settings".to_string(),
            title: "Settings".to_string(),
            items: vec![SelectItem {
                label: "thinking".to_string(),
                detail: None,
                value: "thinking".to_string(),
            }],
        });
        state.on_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL));

        assert_eq!(
            state.take_pending_submissions(),
            vec![FullscreenSubmission::MenuSelection {
                menu_id: "settings".to_string(),
                value: "thinking".to_string(),
            }]
        );
    }

    #[test]
    fn keyboard_navigation_turns_follow_off_and_resize_preserves_focus_anchor_when_possible() {
        let (mut state, _) = scrolling_state();

        state.on_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL));
        assert!(state.viewport.auto_follow);

        state.on_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        let focused = state.focused_block.expect("focus after navigation");
        let header_row = state
            .projection
            .header_row_for_block(focused)
            .expect("focused header row should exist");
        let anchor_offset = header_row.saturating_sub(state.viewport.viewport_top);

        assert!(!state.viewport.auto_follow);

        state.on_resize(72, 14);

        let resized_header_row = state
            .projection
            .header_row_for_block(focused)
            .expect("focused header row should still exist");
        let expected_top = resized_header_row
            .saturating_sub(anchor_offset)
            .min(state.viewport.bottom_top());
        assert_eq!(state.focused_block, Some(focused));
        assert_eq!(state.viewport.viewport_top, expected_top);
        assert!(resized_header_row >= state.viewport.viewport_top);
        assert!(resized_header_row < state.viewport.viewport_top + state.viewport.viewport_height);
    }

    #[test]
    fn cursor_is_only_visible_in_normal_mode() {
        let (mut state, _, _, _) = sample_state();
        state.input = "hello".to_string();
        state.cursor = state.input.len();

        assert!(build_frame(&state).cursor.is_some());

        state.on_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL));
        assert!(build_frame(&state).cursor.is_none());

        state.on_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        assert_eq!(state.mode, FullscreenMode::Search);
        assert!(build_frame(&state).cursor.is_none());
    }

    #[test]
    fn turn_end_returns_to_normal_mode_when_auto_following() {
        let mut state = FullscreenState::new(
            FullscreenAppConfig::default(),
            Size { width: 80, height: 24 },
        );
        // Start a turn
        let _ = state.apply_command(FullscreenCommand::TurnStart { turn_index: 0 });
        assert!(state.has_active_turn());
        assert_eq!(state.mode, FullscreenMode::Normal);

        // End the turn — should stay in Normal mode
        let _ = state.apply_command(FullscreenCommand::TurnEnd);
        assert!(!state.has_active_turn());
        assert_eq!(state.mode, FullscreenMode::Normal);
        assert!(state.status_line.trim().is_empty(), "status should be cleared");
    }

    #[test]
    fn turn_end_stays_in_transcript_when_user_scrolled_away() {
        let mut state = FullscreenState::new(
            FullscreenAppConfig::default(),
            Size { width: 80, height: 24 },
        );
        let _ = state.apply_command(FullscreenCommand::TurnStart { turn_index: 0 });

        // User scrolls up → enters Transcript mode, auto_follow off
        state.mode = FullscreenMode::Transcript;
        state.viewport.auto_follow = false;

        let _ = state.apply_command(FullscreenCommand::TurnEnd);
        // Should stay in Transcript since user explicitly scrolled away
        assert_eq!(state.mode, FullscreenMode::Transcript);
    }

    #[test]
    fn printable_char_in_transcript_mode_switches_to_normal_and_inserts() {
        let mut state = FullscreenState::new(
            FullscreenAppConfig::default(),
            Size { width: 80, height: 24 },
        );
        state.mode = FullscreenMode::Transcript;

        // Press 'a' — should switch to Normal and insert 'a'
        state.on_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        assert_eq!(state.mode, FullscreenMode::Normal);
        assert_eq!(state.input, "a");
    }
}
