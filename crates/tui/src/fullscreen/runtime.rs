use std::collections::VecDeque;
use std::io;
use std::time::{Duration, Instant};

use tokio::sync::mpsc;

use crate::select_list::SelectItem;

use super::{
    frame::{build_frame, measure_input},
    menus::{FullscreenSelectMenuState, FullscreenSlashMenuState},
    layout::{FullscreenLayout, Size, compute_layout_with_footer},
    projection::{TranscriptProjection, TranscriptProjector},
    renderer::FullscreenRenderer,
    scheduler::{RenderIntent, RenderScheduler},
    streaming::{ActiveTurnState, ToolCallState},
    terminal::{FullscreenEvent, FullscreenTerminal, spawn_event_reader},
    transcript::{BlockId, BlockKind, NewBlock, Transcript},
    types::{
        FullscreenAppConfig, FullscreenCommand, FullscreenFooterData, FullscreenMode,
        FullscreenNoteLevel, FullscreenOutcome, FullscreenSearchState, FullscreenSubmission,
    },
    viewport::ViewportState,
};

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
    pub(super) dirty: bool,
    pub(super) should_quit: bool,
    pub tick_count: u64,
    pub(super) submitted_inputs: Vec<String>,
    projector: TranscriptProjector,
    pub(crate) slash_menu: Option<FullscreenSlashMenuState>,
    pub(super) select_menu: Option<FullscreenSelectMenuState>,
    pub(crate) projection_dirty: bool,
    pub(super) pending_submissions: VecDeque<FullscreenSubmission>,
    pub(super) active_turn: Option<ActiveTurnState>,
    pub(super) expanded_tool_blocks: std::collections::HashSet<BlockId>,
    /// Persistent tool call state — survives after active_turn is cleared.
    pub(super) all_tool_states: std::collections::HashMap<String, super::streaming::ToolCallState>,
    pub(crate) extra_slash_items: Vec<SelectItem>,
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
            expanded_tool_blocks: std::collections::HashSet::new(),
            all_tool_states: std::collections::HashMap::new(),
            extra_slash_items: config.extra_slash_items,
        };
        state.prepare_for_render();
        state
    }

    #[allow(dead_code)]
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    #[allow(dead_code)]
    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    #[allow(dead_code)]
    pub fn take_submitted_inputs(&mut self) -> Vec<String> {
        std::mem::take(&mut self.submitted_inputs)
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
        self.active_turn
            .as_ref()
            .is_some_and(|turn| !turn.finished)
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
                // Clear previous active turn (tool results already processed)
                self.active_turn = None;
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
                // Save to persistent map for post-turn re-rendering
                if let Some(tool) = self.tool_call_state(&id).cloned() {
                    self.all_tool_states.insert(id.clone(), tool);
                }
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

    pub(super) fn mode_help_text(&self) -> String {
        match self.mode {
            FullscreenMode::Normal => String::new(),
            FullscreenMode::Transcript => {
                "transcript mode • j/k navigate • Enter/Space toggle • o expand • c collapse • Ctrl+O tool output • Esc returns"
                    .to_string()
            }
            FullscreenMode::Search => {
                // Search mode is no longer reachable from transcript.
                String::new()
            }
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



    pub(super) fn toggle_tool_output_expansion(&mut self) {
        // Toggle the focused tool block's expansion state
        let block_id = match self.focused_block {
            Some(id) => id,
            None => {
                self.status_line = "no block focused".to_string();
                self.dirty = true;
                return;
            }
        };

        // Find the tool_use block (could be the block itself or its parent)
        let tool_use_id = if self
            .transcript
            .block(block_id)
            .is_some_and(|b| b.kind == super::transcript::BlockKind::ToolUse)
        {
            block_id
        } else if let Some(parent_id) = self
            .transcript
            .block(block_id)
            .and_then(|b| b.parent)
        {
            if self
                .transcript
                .block(parent_id)
                .is_some_and(|b| b.kind == super::transcript::BlockKind::ToolUse)
            {
                parent_id
            } else {
                self.status_line = "not a tool block".to_string();
                self.dirty = true;
                return;
            }
        } else {
            self.status_line = "not a tool block".to_string();
            self.dirty = true;
            return;
        };

        if self.expanded_tool_blocks.contains(&tool_use_id) {
            self.expanded_tool_blocks.remove(&tool_use_id);
        } else {
            self.expanded_tool_blocks.insert(tool_use_id);
        }

        // Try re-rendering through active turn state (works during streaming)
        if let Some(active_turn) = self.active_turn.as_ref() {
            let ids = active_turn
                .tools
                .iter()
                .filter(|(_, t)| t.tool_use_id == tool_use_id)
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>();
            for id in ids {
                self.refresh_tool_rendering(&id);
            }
        }

        // Re-render from persistent tool state (works after turn finishes)
        let expanded = self.expanded_tool_blocks.contains(&tool_use_id);
        // Find the tool_call_id that maps to this tool_use_id
        let tool_ids: Vec<String> = self
            .all_tool_states
            .iter()
            .filter(|(_, t)| t.tool_use_id == tool_use_id)
            .map(|(id, _)| id.clone())
            .collect();
        for tool_id in tool_ids {
            if let Some(tool) = self.all_tool_states.get(&tool_id).cloned() {
                let _ = self.transcript.replace_content(
                    tool.tool_use_id,
                    super::tool_format::format_tool_call_content(
                        &tool.name,
                        &tool.raw_args,
                        expanded,
                    ),
                );
                if let (Some(result_id), Some(content)) =
                    (tool.tool_result_id, tool.result_content.as_ref())
                {
                    let formatted = super::tool_format::format_tool_result_content(
                        &tool.name,
                        content,
                        tool.result_details.clone(),
                        tool.artifact_path.clone(),
                        tool.is_error,
                        expanded,
                    );
                    let _ = self.transcript.replace_tool_result_content(result_id, formatted);
                }
            }
        }
        self.projection_dirty = true;
        self.dirty = true;
    }

    pub(super) fn is_tool_block_expanded(&self, tool_use_id: BlockId) -> bool {
        self.expanded_tool_blocks.contains(&tool_use_id)
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
                        // Controller exited (e.g. /quit) — exit the TUI too.
                        state.should_quit = true;
                        state.dirty = true;
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
