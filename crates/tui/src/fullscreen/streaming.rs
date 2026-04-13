use std::collections::HashMap;

use bb_core::types::ContentBlock;

use crate::ui_hints::{
    image_placeholder, NO_TEXT_OUTPUT, TOOL_COLLAPSE_HINT, TOOL_EXPAND_HINT,
    TOOL_FAILED_NO_TEXT_OUTPUT,
};

use super::{
    projection::wrap_visual_preview_lines,
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
    pub(super) started_tick: u64,
    pub(super) started_at: Option<std::time::Instant>,
}

impl ActiveTurnState {
    pub(super) fn new(root_id: BlockId, turn_index: u32, started_tick: u64) -> Self {
        Self {
            root_id,
            turn_index,
            thinking_id: None,
            content_id: None,
            tools: HashMap::new(),
            finished: false,
            started_tick,
            started_at: Some(std::time::Instant::now()),
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
    pub(super) started_tick: Option<u64>,
    pub(super) started_at: Option<std::time::Instant>,
    pub(super) finished_duration_ms: Option<u64>,
    pub(super) live_output: String,
    pub(super) result_content: Option<Vec<ContentBlock>>,
    pub(super) result_details: Option<serde_json::Value>,
    pub(super) artifact_path: Option<String>,
    pub(super) is_error: bool,
}

const TOOL_TIMER_TICK_MS: u64 = 80;
const LIVE_TOOL_OUTPUT_MAX_BYTES: usize = 16 * 1024;
const LIVE_BASH_PREVIEW_VISUAL_LINES: usize = 5;

impl ToolCallState {
    pub(super) fn append_live_output(&mut self, chunk: &str) {
        self.live_output.push_str(chunk);
        if self.live_output.len() <= LIVE_TOOL_OUTPUT_MAX_BYTES {
            return;
        }

        let overflow = self
            .live_output
            .len()
            .saturating_sub(LIVE_TOOL_OUTPUT_MAX_BYTES);
        let mut trim_at = self
            .live_output
            .char_indices()
            .find(|(idx, _)| *idx >= overflow)
            .map(|(idx, _)| idx)
            .unwrap_or(0);
        if let Some(next_newline) = self.live_output[trim_at..].find('\n') {
            trim_at += next_newline + 1;
        }
        self.live_output.drain(..trim_at);
    }
}

pub(super) fn format_elapsed_ms(ms: u64) -> String {
    if ms < 1_000 {
        format!("{ms}ms")
    } else if ms >= 60_000 {
        format!("{:.1}m", ms as f64 / 60_000.0)
    } else {
        format!("{:.1}s", ms as f64 / 1000.0)
    }
}

fn format_bash_footer(
    label: &str,
    elapsed: Option<&str>,
    hidden_visual_lines: usize,
    expanded: bool,
) -> String {
    let mut parts = Vec::new();
    parts.push(format!("{label} {}", elapsed.unwrap_or("0ms")));
    if hidden_visual_lines > 0 {
        parts.push(format!("{hidden_visual_lines} earlier lines hidden"));
    }
    parts.push(if expanded {
        TOOL_COLLAPSE_HINT.to_string()
    } else {
        TOOL_EXPAND_HINT.to_string()
    });
    parts.join(" • ")
}

fn bash_text_output(content: &[ContentBlock]) -> String {
    let mut parts = Vec::new();
    for block in content {
        match block {
            ContentBlock::Text { text } => parts.push(text.clone()),
            ContentBlock::Image { mime_type, .. } => parts.push(image_placeholder(mime_type)),
        }
    }
    parts.join("\n")
}

fn bash_status_lines(details: Option<&serde_json::Value>) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(details) = details {
        let exit = details.get("exitCode").and_then(|v| v.as_i64());
        let truncated = details
            .get("truncated")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let cancelled = details
            .get("cancelled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let mut flags = Vec::new();
        if truncated {
            flags.push("truncated");
        }
        if cancelled {
            flags.push("cancelled");
        }
        match exit {
            Some(exit) if exit != 0 || !flags.is_empty() => {
                let suffix = if flags.is_empty() {
                    String::new()
                } else {
                    format!(" [{}]", flags.join(", "))
                };
                lines.push(format!("exit code: {exit}{suffix}"));
            }
            None if !flags.is_empty() => {
                lines.push(format!("status: {}", flags.join(", ")));
            }
            _ => {}
        }
    }
    lines
}

pub(super) fn format_bash_visual_result_content(
    label: &str,
    content: &[ContentBlock],
    details: Option<&serde_json::Value>,
    artifact_path: Option<&str>,
    is_error: bool,
    expanded: bool,
    total_width: usize,
    elapsed: Option<&str>,
) -> String {
    let available_width = total_width.saturating_sub(2).max(1);
    let mut out = bash_status_lines(details);
    let text = bash_text_output(content).replace('\t', "   ");

    if !text.trim().is_empty() {
        let visual_lines = wrap_visual_preview_lines(&text, available_width);
        let hidden_visual_lines = if expanded {
            0
        } else {
            visual_lines
                .len()
                .saturating_sub(LIVE_BASH_PREVIEW_VISUAL_LINES)
        };
        let visible_lines = if expanded {
            visual_lines
        } else {
            visual_lines
                .into_iter()
                .skip(hidden_visual_lines)
                .collect::<Vec<_>>()
        };
        if !out.is_empty() && !visible_lines.is_empty() {
            out.push(String::new());
        }
        out.extend(visible_lines);
        out.push(format_bash_footer(
            label,
            elapsed,
            hidden_visual_lines,
            expanded,
        ));
    } else if out.is_empty() {
        out.push(if is_error {
            TOOL_FAILED_NO_TEXT_OUTPUT.to_string()
        } else {
            NO_TEXT_OUTPUT.to_string()
        });
    } else {
        out.push(format_bash_footer(label, elapsed, 0, expanded));
    }

    if let Some(path) = artifact_path {
        if !out.is_empty() {
            out.push(String::new());
        }
        out.push(format!(
            "artifact: {}",
            super::tool_format::shorten_display_path(path)
        ));
    }

    out.join("\n")
}

fn format_live_bash_result_content(
    live_output: &str,
    expanded: bool,
    total_width: usize,
    elapsed: Option<&str>,
) -> String {
    format_bash_visual_result_content(
        "Elapsed",
        &[ContentBlock::Text {
            text: live_output.to_string(),
        }],
        None,
        None,
        false,
        expanded,
        total_width,
        elapsed,
    )
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

    fn tool_elapsed_ms(&self, tool: &ToolCallState) -> Option<u64> {
        if let Some(ms) = tool.finished_duration_ms {
            return Some(ms);
        }
        if let Some(ms) = tool
            .result_details
            .as_ref()
            .and_then(|details| details.get("durationMs"))
            .and_then(|value| value.as_u64())
        {
            return Some(ms);
        }
        if let Some(started_at) = tool.started_at {
            return Some(started_at.elapsed().as_millis() as u64);
        }
        tool.started_tick
            .map(|started| self.tick_count.saturating_sub(started) * TOOL_TIMER_TICK_MS)
    }

    pub(super) fn local_action_elapsed_ms(&self) -> Option<u64> {
        if let Some(started_at) = self.local_action_started_at {
            return Some(started_at.elapsed().as_millis() as u64);
        }
        self.local_action_started_tick
            .map(|started| self.tick_count.saturating_sub(started) * TOOL_TIMER_TICK_MS)
    }

    pub(super) fn local_action_status_message(&self) -> Option<String> {
        if !self.local_action_active {
            return None;
        }
        let base = self.status_line.trim();
        if base.is_empty() {
            return None;
        }
        let elapsed = self
            .local_action_elapsed_ms()
            .map(format_elapsed_ms)
            .unwrap_or_else(|| "0.0s".to_string());
        Some(format!("{base} • {elapsed}"))
    }

    fn active_turn_elapsed_ms(&self) -> Option<u64> {
        let active_turn = self.active_turn.as_ref()?;
        if active_turn.finished {
            return None;
        }
        active_turn
            .started_at
            .map(|started_at| started_at.elapsed().as_millis() as u64)
            .or_else(|| {
                Some(self.tick_count.saturating_sub(active_turn.started_tick) * TOOL_TIMER_TICK_MS)
            })
    }

    pub(super) fn active_turn_status_message(&self) -> Option<String> {
        if let Some(message) = self.running_tool_status_message() {
            return Some(message);
        }

        let active_turn = self.active_turn.as_ref().filter(|turn| !turn.finished)?;
        let elapsed = self
            .active_turn_elapsed_ms()
            .map(format_elapsed_ms)
            .unwrap_or_else(|| "0.0s".to_string());

        if active_turn.content_id.is_some() {
            return Some(format!("writing • {elapsed}"));
        }
        if active_turn.thinking_id.is_some() {
            return Some(format!("thinking • {elapsed}"));
        }
        if let Some(tool) = active_turn
            .tools
            .values()
            .find(|tool| !tool.execution_started && tool.result_content.is_none())
        {
            let display_name = format_tool_call_title(&tool.name, &tool.raw_args);
            return Some(format!("preparing {display_name} • {elapsed}"));
        }
        Some(format!("requesting response • {elapsed}"))
    }

    pub(super) fn running_tool_status_message(&self) -> Option<String> {
        let active_turn = self.active_turn.as_ref()?;
        let tool = active_turn
            .tools
            .values()
            .find(|tool| tool.execution_started && tool.result_content.is_none())?;
        let display_name = format_tool_call_title(&tool.name, &tool.raw_args);
        let elapsed = self
            .tool_elapsed_ms(tool)
            .map(format_elapsed_ms)
            .unwrap_or_else(|| "0.0s".to_string());
        Some(format!("running {display_name} • {elapsed}"))
    }

    pub(super) fn refresh_running_tool_visuals(&mut self) {
        let running_ids = self
            .active_turn
            .as_ref()
            .map(|turn| {
                turn.tools
                    .iter()
                    .filter(|(_, tool)| tool.execution_started && tool.result_content.is_none())
                    .map(|(id, _)| id.clone())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        for id in running_ids {
            self.refresh_tool_rendering(&id);
        }
        if let Some(message) = self.running_tool_status_message() {
            self.status_line = message;
            self.dirty = true;
        }
    }

    pub(super) fn refresh_tool_rendering(&mut self, id: &str) {
        let Some(tool) = self.tool_call_state(id).cloned() else {
            return;
        };

        let display_name = format_tool_call_title(&tool.name, &tool.raw_args);
        let elapsed = self.tool_elapsed_ms(&tool).map(format_elapsed_ms);
        let title = if tool.result_content.is_some() {
            let status = if tool.is_error { "error" } else { "done" };
            if let Some(elapsed) = elapsed.as_deref() {
                format!("{display_name} • {status} in {elapsed}")
            } else {
                format!("{display_name} • {status}")
            }
        } else if tool.execution_started {
            if let Some(elapsed) = elapsed.as_deref() {
                format!("{display_name} • running {elapsed}")
            } else {
                format!("{display_name} • running")
            }
        } else {
            display_name
        };
        let _ = self.transcript.update_title(tool.tool_use_id, title);
        let expanded = self.is_tool_block_expanded(tool.tool_use_id);
        let tool_use_content = if tool.result_content.is_some() {
            format_tool_call_content(&tool.name, &tool.raw_args, expanded)
        } else {
            String::new()
        };
        let _ = self
            .transcript
            .replace_content(tool.tool_use_id, tool_use_content);

        if let Some(result_content) = tool.result_content.clone() {
            let Some(tool_result_id) = self.ensure_tool_result_block(id) else {
                return;
            };
            let _ = self.transcript.update_title(
                tool_result_id,
                if tool.is_error {
                    "error output"
                } else {
                    "output"
                },
            );
            let formatted = if tool.name == "bash" {
                format_bash_visual_result_content(
                    "Took",
                    &result_content,
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
                    &result_content,
                    tool.result_details.clone(),
                    tool.artifact_path.clone(),
                    tool.is_error,
                    expanded,
                )
            };
            let _ = self
                .transcript
                .replace_tool_result_content(tool_result_id, formatted);
        } else if !tool.live_output.trim().is_empty() {
            let Some(tool_result_id) = self.ensure_tool_result_block(id) else {
                return;
            };
            let _ = self.transcript.update_title(tool_result_id, "live output");
            let formatted = if tool.name == "bash" {
                format_live_bash_result_content(
                    &tool.live_output,
                    expanded,
                    self.size.width as usize,
                    elapsed.as_deref(),
                )
            } else {
                format_tool_result_content(
                    &tool.name,
                    &[ContentBlock::Text {
                        text: tool.live_output.clone(),
                    }],
                    None,
                    None,
                    false,
                    expanded,
                )
            };
            let _ = self
                .transcript
                .replace_tool_result_content(tool_result_id, formatted);
        }

        self.projection_dirty = true;
        self.dirty = true;
    }
}
