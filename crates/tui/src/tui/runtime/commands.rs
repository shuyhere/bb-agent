use super::*;
use crate::tui::streaming::{
    ActiveTurnState, ToolCallState, format_bash_visual_result_content, format_elapsed_ms,
};
use crate::tui::types::{
    TuiCommand, TuiMode, TuiNoteLevel, TuiSearchState,
};
use crate::tui::{BlockKind, NewBlock};
use crate::tui::{
    format_tool_call_content, format_tool_call_title, format_tool_result_content,
};

impl TuiState {
    pub fn apply_command(&mut self, command: TuiCommand) -> RenderIntent {
        match command {
            TuiCommand::SetStatusLine(status) => {
                self.status_line = status;
                self.dirty = true;
                RenderIntent::Render
            }
            TuiCommand::SetFooter(footer) => {
                self.footer = footer;
                self.dirty = true;
                RenderIntent::Render
            }
            TuiCommand::SetTranscript(transcript) => {
                self.reset_transcript_state();
                self.transcript = transcript;
                self.projection_dirty = true;
                self.dirty = true;
                RenderIntent::Render
            }
            TuiCommand::SetTranscriptWithToolStates {
                transcript,
                tool_states,
            } => {
                self.reset_transcript_state();
                self.transcript = transcript;
                self.all_tool_states = tool_states
                    .into_iter()
                    .map(|(id, tool)| {
                        (
                            id,
                            ToolCallState {
                                name: tool.name,
                                raw_args: tool.raw_args,
                                tool_use_id: tool.tool_use_id,
                                tool_result_id: tool.tool_result_id,
                                execution_started: false,
                                started_tick: None,
                                started_at: None,
                                finished_duration_ms: None,
                                live_output: String::new(),
                                result_content: tool.result_content,
                                result_details: tool.result_details,
                                artifact_path: tool.artifact_path,
                                is_error: tool.is_error,
                            },
                        )
                    })
                    .collect();
                self.restore_historical_tool_rendering();
                self.projection_dirty = true;
                self.dirty = true;
                RenderIntent::Render
            }
            TuiCommand::SetInput(input) => {
                self.input = input;
                self.cursor = self.input.len();
                self.slash_menu = None;
                self.select_menu = None;
                self.tree_menu = None;
                self.dirty = true;
                RenderIntent::Render
            }
            TuiCommand::SetLocalActionActive(active) => {
                self.local_action_active = active;
                if active {
                    if self.local_action_started_tick.is_none() {
                        self.local_action_started_tick = Some(self.tick_count);
                    }
                    if self.local_action_started_at.is_none() {
                        self.local_action_started_at = Some(std::time::Instant::now());
                    }
                } else {
                    self.local_action_started_tick = None;
                    self.local_action_started_at = None;
                    self.queued_submission_previews.clear();
                    self.editing_queued_messages = false;
                }
                self.dirty = true;
                RenderIntent::Render
            }
            TuiCommand::OpenAuthDialog(dialog)
            | TuiCommand::UpdateAuthDialog(dialog) => {
                self.approval_dialog = None;
                self.auth_dialog = Some(dialog);
                self.dirty = true;
                RenderIntent::Render
            }
            TuiCommand::CloseAuthDialog => {
                self.auth_dialog = None;
                self.dirty = true;
                RenderIntent::Render
            }
            TuiCommand::OpenApprovalDialog(dialog) => {
                self.auth_dialog = None;
                self.approval_dialog = Some(dialog);
                self.dirty = true;
                RenderIntent::Render
            }
            TuiCommand::CloseApprovalDialog => {
                self.approval_dialog = None;
                self.dirty = true;
                RenderIntent::Render
            }
            TuiCommand::SetExtraSlashItems(items) => {
                self.extra_slash_items = items;
                self.slash_menu = None;
                self.update_slash_menu();
                self.dirty = true;
                RenderIntent::Render
            }
            TuiCommand::OpenSelectMenu {
                menu_id,
                title,
                items,
                selected_value,
            } => {
                self.slash_menu = None;
                self.tree_menu = None;
                self.select_menu = Some(TuiSelectMenuState::new(
                    menu_id,
                    title,
                    items,
                    selected_value,
                ));
                self.dirty = true;
                RenderIntent::Render
            }
            TuiCommand::OpenTreeMenu {
                menu_id,
                title: _,
                tree,
                entries,
                active_leaf,
                selected_value,
            } => {
                self.slash_menu = None;
                self.select_menu = None;
                self.tree_menu = Some(super::super::menus::TuiTreeMenuState::new(
                    menu_id,
                    tree,
                    entries,
                    active_leaf,
                    selected_value,
                    self.tree_menu_max_visible(),
                ));
                self.dirty = true;
                RenderIntent::Render
            }
            TuiCommand::CloseSelectMenu => {
                self.select_menu = None;
                self.dirty = true;
                RenderIntent::Render
            }
            TuiCommand::CloseTreeMenu => {
                self.tree_menu = None;
                self.dirty = true;
                RenderIntent::Render
            }
            TuiCommand::PushNote { level, text } => {
                let title = match level {
                    TuiNoteLevel::Status => "status",
                    TuiNoteLevel::Highlight => "highlight",
                    TuiNoteLevel::Warning => "warning",
                    TuiNoteLevel::Error => "error",
                };
                self.transcript.append_root_block(
                    NewBlock::new(BlockKind::SystemNote, title).with_content(text),
                );
                self.projection_dirty = true;
                self.dirty = true;
                RenderIntent::Render
            }
            TuiCommand::TurnStart { turn_index } => {
                self.active_turn = None;
                self.spinner
                    .set_mode(super::super::spinner::SpinnerMode::Requesting);
                self.spinner.notify_activity();
                self.status_line = "Requesting response...".to_string();
                let root_id = self.transcript.append_root_block(
                    NewBlock::new(
                        BlockKind::AssistantMessage,
                        format!("turn {} • streaming", turn_index + 1),
                    )
                    .with_expandable(true),
                );
                self.active_turn = Some(ActiveTurnState::new(root_id, turn_index, self.tick_count));
                self.projection_dirty = true;
                self.dirty = true;
                RenderIntent::Render
            }
            TuiCommand::TextDelta(text) => {
                self.spinner.notify_activity();
                self.spinner
                    .set_mode(super::super::spinner::SpinnerMode::Thinking);
                if text.is_empty() {
                    return RenderIntent::None;
                }
                self.status_line = "Writing...".to_string();
                let Ok(content_id) = self.ensure_assistant_content_block() else {
                    return RenderIntent::None;
                };
                let _ = self.transcript.append_streamed_content(content_id, text);
                self.projection_dirty = true;
                self.dirty = true;
                RenderIntent::Schedule
            }
            TuiCommand::ThinkingDelta(text) => {
                self.spinner.notify_activity();
                self.spinner
                    .set_mode(super::super::spinner::SpinnerMode::Thinking);
                if text.is_empty() {
                    return RenderIntent::None;
                }
                self.status_line = "Thinking...".to_string();
                let Ok(thinking_id) = self.ensure_thinking_block() else {
                    return RenderIntent::None;
                };
                let _ = self.transcript.append_streamed_content(thinking_id, text);
                self.projection_dirty = true;
                self.dirty = true;
                RenderIntent::Schedule
            }
            TuiCommand::ToolCallStart { id, name } => {
                self.spinner.notify_activity();
                self.spinner
                    .set_mode(super::super::spinner::SpinnerMode::Thinking);
                let Some(turn_root_id) = self.ensure_active_turn_root() else {
                    return RenderIntent::None;
                };
                let initial_title = format_tool_call_title(&name, "");
                self.status_line = format!("Preparing {initial_title}...");
                let Ok(tool_use_id) = self.transcript.append_child_block(
                    turn_root_id,
                    NewBlock::new(BlockKind::ToolUse, initial_title).with_expandable(true),
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
                            started_tick: None,
                            started_at: None,
                            finished_duration_ms: None,
                            live_output: String::new(),
                            result_content: None,
                            result_details: None,
                            artifact_path: None,
                            is_error: false,
                        },
                    );
                }
                self.projection_dirty = true;
                self.dirty = true;
                RenderIntent::Render
            }
            TuiCommand::ToolCallDelta { id, args } => {
                self.spinner.notify_activity();
                self.spinner
                    .set_mode(super::super::spinner::SpinnerMode::Thinking);
                if args.is_empty() {
                    return RenderIntent::None;
                }
                let display_name = match self.tool_call_state_mut(&id) {
                    Some(tool) => {
                        tool.raw_args.push_str(&args);
                        format_tool_call_title(&tool.name, &tool.raw_args)
                    }
                    None => return RenderIntent::None,
                };
                self.status_line = format!("Preparing {display_name}...");
                self.refresh_tool_rendering(&id);
                RenderIntent::Render
            }
            TuiCommand::ToolExecuting { id } => {
                self.spinner.notify_activity();
                let tick_count = self.tick_count;
                let Some(tool) = self.tool_call_state_mut(&id) else {
                    return RenderIntent::None;
                };
                tool.execution_started = true;
                if tool.started_tick.is_none() {
                    tool.started_tick = Some(tick_count);
                }
                if tool.started_at.is_none() {
                    tool.started_at = Some(std::time::Instant::now());
                }
                self.refresh_tool_rendering(&id);
                if let Some(message) = self.running_tool_status_message() {
                    self.status_line = message;
                }
                RenderIntent::Render
            }
            TuiCommand::ToolOutputDelta { id, chunk } => {
                if chunk.is_empty() {
                    return RenderIntent::None;
                }
                self.spinner.notify_activity();
                let Some(tool) = self.tool_call_state_mut(&id) else {
                    return RenderIntent::None;
                };
                tool.append_live_output(&chunk);
                self.refresh_tool_rendering(&id);
                if let Some(message) = self.running_tool_status_message() {
                    self.status_line = message;
                }
                RenderIntent::Schedule
            }
            TuiCommand::ToolResult {
                id,
                name: _,
                content,
                details,
                artifact_path,
                is_error,
            } => {
                self.spinner.notify_activity();
                let tick_count = self.tick_count;
                let Some(tool) = self.tool_call_state_mut(&id) else {
                    return RenderIntent::None;
                };
                tool.live_output.clear();
                tool.result_content = Some(content);
                tool.result_details = details;
                tool.artifact_path = artifact_path;
                tool.is_error = is_error;
                if tool.finished_duration_ms.is_none() {
                    let duration_from_details = tool
                        .result_details
                        .as_ref()
                        .and_then(|details| details.get("durationMs"))
                        .and_then(|value| value.as_u64());
                    let duration_from_instant = tool
                        .started_at
                        .map(|started_at| started_at.elapsed().as_millis() as u64);
                    let duration_from_ticks = tool
                        .started_tick
                        .map(|started| tick_count.saturating_sub(started) * 80);
                    tool.finished_duration_ms = duration_from_details
                        .or(duration_from_instant)
                        .or(duration_from_ticks);
                }
                self.refresh_tool_rendering(&id);
                if let Some(message) = self.running_tool_status_message() {
                    self.status_line = message;
                }
                if let Some(tool) = self.tool_call_state(&id).cloned() {
                    self.all_tool_states.insert(id.clone(), tool);
                }
                self.force_full_repaint = true;
                RenderIntent::Render
            }
            TuiCommand::TurnEnd => {
                self.force_full_repaint = true;
                self.finish_active_turn("complete");
                RenderIntent::Render
            }
            TuiCommand::TurnAborted => {
                self.force_full_repaint = true;
                self.finish_active_turn("aborted");
                RenderIntent::Render
            }
            TuiCommand::TurnError { message } => {
                self.status_line = message;
                self.force_full_repaint = true;
                self.finish_active_turn("error");
                RenderIntent::Render
            }
            TuiCommand::SetColorTheme(theme) => {
                self.color_theme = theme;
                self.spinner.set_color_theme(theme);
                self.projection_dirty = true;
                self.dirty = true;
                RenderIntent::Render
            }
        }
    }

    fn reset_transcript_state(&mut self) {
        self.active_turn = None;
        self.all_tool_states.clear();
        self.expanded_tool_blocks.clear();
        self.focused_block = None;
        self.search = TuiSearchState::default();
        self.mode = TuiMode::Normal;
        self.viewport.auto_follow = true;
        self.selection_anchor_row = None;
        self.selection_anchor_col = None;
        self.selection_focus_row = None;
        self.selection_focus_col = None;
        self.tree_menu = None;
    }

    pub(crate) fn mode_help_text(&self) -> String {
        match self.mode {
            TuiMode::Normal => String::new(),
            TuiMode::Transcript => {
                "tool expand mode • j/k or ↑/↓ select tool call • Enter expand/collapse • Esc returns"
                    .to_string()
            }
        }
    }

    pub(crate) fn current_layout(&self) -> TuiLayout {
        let input_inner_width = self.size.width.max(1) as usize;
        let requested_input_lines = if let Some(dialog) = self.approval_dialog.as_ref() {
            crate::tui::frame::measure_approval_input(dialog, input_inner_width)
        } else {
            let (visible_input, visible_cursor) =
                crate::tui::frame::visible_input_text(&self.input, self.cursor, &self.cwd);
            crate::tui::frame::attachment_line_count(self, input_inner_width)
                + measure_input(&visible_input, visible_cursor, input_inner_width)
                    .lines
                    .len()
        };
        compute_layout_with_footer(
            self.size,
            requested_input_lines,
            self.requested_footer_height(),
        )
    }

    pub(crate) fn requested_footer_height(&self) -> u16 {
        if self.tree_menu.is_some() {
            self.size
                .height
                .saturating_sub(if self.size.height >= 8 { 4 } else { 1 })
        } else if let Some(menu) = self.select_menu.as_ref() {
            menu.rendered_height()
        } else if let Some(menu) = self.slash_menu.as_ref() {
            menu.rendered_height()
        } else if let Some(menu) = self.at_file_menu.as_ref() {
            menu.rendered_height()
        } else if self.size.height >= 14 {
            2
        } else {
            0
        }
    }

    fn tree_menu_max_visible(&self) -> usize {
        self.size
            .height
            .saturating_sub(if self.size.height >= 8 { 8 } else { 3 }) as usize
    }

    pub(crate) fn toggle_tool_output_expansion(&mut self) {
        let block_id = match self.focused_block {
            Some(id) => id,
            None => {
                self.status_line = crate::ui_hints::NO_BLOCK_FOCUSED_HINT.to_string();
                self.dirty = true;
                return;
            }
        };

        let tool_use_id = if self
            .transcript
            .block(block_id)
            .is_some_and(|block| block.kind == super::super::transcript::BlockKind::ToolUse)
        {
            block_id
        } else if let Some(parent_id) = self
            .transcript
            .block(block_id)
            .and_then(|block| block.parent)
        {
            if self
                .transcript
                .block(parent_id)
                .is_some_and(|block| block.kind == super::super::transcript::BlockKind::ToolUse)
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

        let should_expand = !self.expanded_tool_blocks.contains(&tool_use_id);
        if should_expand {
            self.expanded_tool_blocks.insert(tool_use_id);
        } else {
            self.expanded_tool_blocks.remove(&tool_use_id);
        }

        if let Some(active_turn) = self.active_turn.as_ref() {
            let ids = active_turn
                .tools
                .iter()
                .filter(|(_, tool)| tool.tool_use_id == tool_use_id)
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>();
            for id in ids {
                self.refresh_tool_rendering(&id);
            }
        }

        let historical_tool_ids = self
            .all_tool_states
            .iter()
            .filter(|(_, tool)| tool.tool_use_id == tool_use_id)
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        for tool_id in historical_tool_ids {
            self.refresh_historical_tool_rendering(&tool_id);
        }
        self.projection_dirty = true;
        self.dirty = true;
    }

    fn restore_historical_tool_rendering(&mut self) {
        let historical_tool_ids = self.all_tool_states.keys().cloned().collect::<Vec<_>>();
        for tool_id in historical_tool_ids {
            self.refresh_historical_tool_rendering(&tool_id);
        }
    }

    fn refresh_historical_tool_rendering(&mut self, tool_id: &str) {
        let Some(tool) = self.all_tool_states.get(tool_id).cloned() else {
            return;
        };
        let expanded = self.expanded_tool_blocks.contains(&tool.tool_use_id);
        let _ = self.transcript.replace_content(
            tool.tool_use_id,
            format_tool_call_content(&tool.name, &tool.raw_args, expanded),
        );
        if let Some(formatted) = self.format_historical_tool_result(&tool, expanded)
            && let Some(result_id) = tool.tool_result_id
        {
            let _ = self
                .transcript
                .replace_tool_result_content(result_id, formatted);
        }
    }

    fn format_historical_tool_result(
        &self,
        tool: &ToolCallState,
        expanded: bool,
    ) -> Option<String> {
        let content = tool.result_content.as_ref()?;
        let elapsed = tool
            .result_details
            .as_ref()
            .and_then(|details| details.get("durationMs"))
            .and_then(|value| value.as_u64())
            .map(format_elapsed_ms);
        Some(if tool.name == "bash" {
            format_bash_visual_result_content(
                "Took",
                content,
                tool.result_details.as_ref(),
                tool.artifact_path.as_deref(),
                tool.is_error,
                expanded,
                self.size.width as usize,
                elapsed.as_deref(),
            )
        } else {
            format_tool_result_content(
                &tool.name,
                content,
                tool.result_details.clone(),
                tool.artifact_path.clone(),
                tool.is_error,
                expanded,
            )
        })
    }

    pub(crate) fn is_tool_block_expanded(&self, tool_use_id: BlockId) -> bool {
        self.expanded_tool_blocks.contains(&tool_use_id)
    }
}
