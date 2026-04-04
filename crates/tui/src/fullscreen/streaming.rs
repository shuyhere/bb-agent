use std::collections::HashMap;

use bb_core::types::ContentBlock;

use super::{
    runtime::FullscreenState,
    tool_format::{format_tool_call_content, format_tool_call_title, format_tool_result_content},
    transcript::{BlockId, BlockKind, NewBlock},
    types::FullscreenMode,
};

#[derive(Clone, Debug)]
pub(super) struct ActiveTurnState {
    pub(super) root_id: BlockId,
    pub(super) turn_index: u32,
    pub(super) thinking_id: Option<BlockId>,
    pub(super) content_id: Option<BlockId>,
    pub(super) tools: HashMap<String, ToolCallState>,
    /// True once TurnEnd has been received. The turn data stays alive
    /// so late-arriving ToolResult events can still be processed.
    pub(super) finished: bool,
}

impl ActiveTurnState {
    pub(super) fn new(root_id: BlockId, turn_index: u32) -> Self {
        Self {
            root_id,
            turn_index,
            thinking_id: None,
            content_id: None,
            tools: HashMap::new(),
            finished: false,
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct ToolCallState {
    pub(super) name: String,
    pub(super) raw_args: String,
    pub(super) tool_use_id: BlockId,
    pub(super) tool_result_id: Option<BlockId>,
    pub(super) execution_started: bool,
    pub(super) result_content: Option<Vec<ContentBlock>>,
    pub(super) result_details: Option<serde_json::Value>,
    pub(super) artifact_path: Option<String>,
    pub(super) is_error: bool,
}

impl FullscreenState {
    pub(super) fn finish_active_turn(&mut self, status: &str) {
        // Mark the turn as finished but keep the data so late-arriving
        // ToolResult events can still find their tool state.
        if let Some(ref mut active_turn) = self.active_turn {
            active_turn.finished = true;
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

    pub(super) fn ensure_active_turn_root(&mut self) -> Option<BlockId> {
        self.active_turn.as_ref().map(|turn| turn.root_id)
    }

    pub(super) fn ensure_assistant_content_block(&mut self) -> Result<BlockId, ()> {
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

    pub(super) fn ensure_thinking_block(&mut self) -> Result<BlockId, ()> {
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

    pub(super) fn ensure_tool_result_block(&mut self, id: &str) -> Option<BlockId> {
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

    pub(super) fn tool_call_state(&self, id: &str) -> Option<&ToolCallState> {
        self.active_turn.as_ref()?.tools.get(id)
    }

    pub(super) fn tool_call_state_mut(&mut self, id: &str) -> Option<&mut ToolCallState> {
        self.active_turn.as_mut()?.tools.get_mut(id)
    }

    pub(super) fn refresh_tool_rendering(&mut self, id: &str) {
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
}
