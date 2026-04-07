mod commands;
mod driver;

use std::collections::VecDeque;

use crate::select_list::SelectItem;

use super::{
    frame::measure_input,
    layout::{FullscreenLayout, Size, compute_layout_with_footer},
    menus::{FullscreenSelectMenuState, FullscreenSlashMenuState},
    projection::{TranscriptProjection, TranscriptProjector},
    scheduler::RenderIntent,
    streaming::ActiveTurnState,
    transcript::{BlockId, Transcript},
    types::{
        FullscreenAppConfig, FullscreenAuthDialog, FullscreenFooterData, FullscreenMode,
        FullscreenOutcome, FullscreenSearchState, FullscreenSubmission,
    },
    viewport::ViewportState,
};

pub use driver::{run, run_with_channels};

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
    pub(super) tree_menu: Option<super::menus::FullscreenTreeMenuState>,
    pub(crate) projection_dirty: bool,
    pub(super) pending_submissions: VecDeque<FullscreenSubmission>,
    pub(super) active_turn: Option<ActiveTurnState>,
    pub(super) local_action_active: bool,
    pub(super) expanded_tool_blocks: std::collections::HashSet<BlockId>,
    /// Persistent tool call state — survives after active_turn is cleared.
    pub(super) all_tool_states: std::collections::HashMap<String, super::streaming::ToolCallState>,
    pub(crate) extra_slash_items: Vec<SelectItem>,
    /// File completion menu triggered by `@`.
    pub(crate) at_file_menu: Option<super::menus::AtFileMenuState>,
    pub(crate) cwd: std::path::PathBuf,
    pub(super) spinner: super::spinner::SpinnerState,
    pub(super) color_theme: super::spinner::ColorTheme,
    pub(super) selection_mode: bool,
    pub(super) auth_dialog: Option<FullscreenAuthDialog>,
    pub(super) selection_anchor_row: Option<usize>,
    pub(super) selection_anchor_col: Option<usize>,
    pub(super) selection_focus_row: Option<usize>,
    pub(super) selection_focus_col: Option<usize>,
    pub(super) pending_clipboard_copy: Option<String>,
    /// Image file paths attached via paste or Ctrl+V, pending next submission.
    pub(super) pending_image_paths: Vec<String>,
    /// Counter for paste markers `[paste #N ...]`.
    pub(super) paste_counter: usize,
    /// Stored content for collapsed large pastes, keyed by paste ID.
    pub(super) paste_storage: std::collections::HashMap<usize, String>,
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
            tree_menu: None,
            projection_dirty: true,
            pending_submissions: VecDeque::new(),
            active_turn: None,
            local_action_active: false,
            expanded_tool_blocks: std::collections::HashSet::new(),
            all_tool_states: std::collections::HashMap::new(),
            extra_slash_items: config.extra_slash_items,
            at_file_menu: None,
            cwd: config.cwd,
            spinner: super::spinner::SpinnerState::new(super::spinner::SpinnerMode::Thinking),
            color_theme: super::spinner::ColorTheme::default(),
            selection_mode: false,
            auth_dialog: None,
            selection_anchor_row: None,
            selection_anchor_col: None,
            selection_focus_row: None,
            selection_focus_col: None,
            pending_clipboard_copy: None,
            pending_image_paths: Vec::new(),
            paste_counter: 0,
            paste_storage: std::collections::HashMap::new(),
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

    pub fn take_pending_clipboard_copy(&mut self) -> Option<String> {
        self.pending_clipboard_copy.take()
    }

    pub(crate) fn has_active_turn(&self) -> bool {
        self.active_turn.as_ref().is_some_and(|turn| !turn.finished)
    }

    pub(crate) fn has_running_tool(&self) -> bool {
        self.active_turn.as_ref().is_some_and(|turn| {
            turn.tools
                .values()
                .any(|tool| tool.execution_started && tool.result_content.is_none())
        })
    }

    pub(crate) fn has_cancellable_action(&self) -> bool {
        self.has_active_turn() || self.has_running_tool() || self.local_action_active
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

        let preserving_focus_anchor = preserve_anchor
            && !self.viewport.auto_follow
            && matches!(self.mode, FullscreenMode::Transcript);
        let anchor = if preserve_anchor && !self.viewport.auto_follow {
            if preserving_focus_anchor {
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

        let next_projection = self.projector.project(
            &mut self.transcript,
            transcript_width,
            &self.expanded_tool_blocks,
        );
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
        if matches!(self.mode, FullscreenMode::Transcript) && self.focused_block.is_none() {
            self.focused_block = self.default_focus_block();
        }
        self.sync_focus_tracking();
        if matches!(self.mode, FullscreenMode::Transcript) {
            let focus_visible = self
                .focused_block
                .and_then(|block_id| self.focus_row_for_block(block_id))
                .is_some_and(|row| {
                    row >= self.viewport.viewport_top
                        && row < self.viewport.viewport_top + self.viewport.viewport_height
                });
            if !preserving_focus_anchor || !focus_visible {
                self.ensure_focus_visible();
            }
        }
        self.projection_dirty = false;
    }
}
