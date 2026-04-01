use bb_tui::utils::visible_width;
use serde_json::Value;

use super::diff_display::render_diff_lines;

const RESET: &str = "\x1b[0m";
const DIM: &str = "\x1b[90m";
const BOLD: &str = "\x1b[1m";
const ACCENT: &str = "\x1b[38;2;178;148;187m";
const SUCCESS: &str = "\x1b[32m";
const ERROR: &str = "\x1b[31m";
const MUTED: &str = "\x1b[38;2;148;163;184m";
const TOOL_PENDING_BG: &str = "\x1b[48;2;40;40;50m";
const TOOL_SUCCESS_BG: &str = "\x1b[48;2;40;50;40m";
const TOOL_ERROR_BG: &str = "\x1b[48;2;60;40;40m";

#[derive(Debug, Clone, Default)]
pub struct ToolExecutionOptions {
    pub show_images: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ToolResultBlock {
    pub r#type: String,
    pub text: Option<String>,
    pub data: Option<String>,
    pub mime_type: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ToolExecutionResult {
    pub content: Vec<ToolResultBlock>,
    pub is_error: bool,
    pub details: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct ToolExecutionComponent {
    tool_name: String,
    tool_call_id: String,
    args: Value,
    expanded: bool,
    show_images: bool,
    is_partial: bool,
    execution_started: bool,
    args_complete: bool,
    result: Option<ToolExecutionResult>,
}

impl ToolExecutionComponent {
    pub fn new(
        tool_name: impl Into<String>,
        tool_call_id: impl Into<String>,
        args: Value,
        options: ToolExecutionOptions,
    ) -> Self {
        Self {
            tool_name: tool_name.into(),
            tool_call_id: tool_call_id.into(),
            args,
            expanded: false,
            show_images: options.show_images,
            is_partial: true,
            execution_started: false,
            args_complete: false,
            result: None,
        }
    }

    pub fn update_args(&mut self, args: Value) {
        self.args = args;
    }

    pub fn mark_execution_started(&mut self) {
        self.execution_started = true;
    }

    pub fn set_args_complete(&mut self) {
        self.args_complete = true;
    }

    pub fn update_result(&mut self, result: ToolExecutionResult, is_partial: bool) {
        self.result = Some(result);
        self.is_partial = is_partial;
    }

    pub fn set_expanded(&mut self, expanded: bool) {
        self.expanded = expanded;
    }

    pub fn set_show_images(&mut self, show: bool) {
        self.show_images = show;
    }

    pub fn tool_name(&self) -> &str {
        &self.tool_name
    }

    pub fn tool_call_id(&self) -> &str {
        &self.tool_call_id
    }

    pub fn args(&self) -> &Value {
        &self.args
    }

    pub fn result(&self) -> Option<&ToolExecutionResult> {
        self.result.as_ref()
    }

    pub fn is_partial(&self) -> bool {
        self.is_partial
    }

    pub fn execution_started(&self) -> bool {
        self.execution_started
    }

    pub fn args_complete(&self) -> bool {
        self.args_complete
    }

    pub fn expanded(&self) -> bool {
        self.expanded
    }

    pub fn render_lines(&self, width: u16) -> Vec<String> {
        let (status_mark, bg) = if let Some(result) = &self.result {
            if result.is_error {
                (format!("{ERROR}x{RESET}"), TOOL_ERROR_BG)
            } else {
                (format!("{SUCCESS}*{RESET}"), TOOL_SUCCESS_BG)
            }
        } else if self.execution_started {
            (format!("{DIM}..{RESET}"), TOOL_PENDING_BG)
        } else {
            (format!("{DIM}>{RESET}"), TOOL_PENDING_BG)
        };

        let title = format!("{status_mark} {BOLD}{}{RESET}", self.render_call_title());
        let mut content = vec![title];
        content.extend(self.render_call_body());

        let result_lines = self.render_result_body();
        if !result_lines.is_empty() {
            if !content.is_empty() {
                content.push(String::new());
            }
            content.extend(result_lines);
        }

        render_box_lines(&content, width, bg)
    }

    fn render_call_title(&self) -> String {
        match self.tool_name.as_str() {
            "read" => format_read_call(&self.args),
            "write" => format_write_call_title(&self.args),
            "edit" => format_edit_call_title(&self.args),
            "bash" => format_bash_call_title(&self.args),
            "ls" => format_ls_call(&self.args),
            "grep" => format_grep_call(&self.args),
            "find" => format_find_call(&self.args),
            other => other.to_string(),
        }
    }

    fn render_call_body(&self) -> Vec<String> {
        match self.tool_name.as_str() {
            "write" => render_write_call_body(&self.args, self.expanded),
            "edit" => render_edit_call_body(&self.args),
            "bash" => render_bash_call_body(&self.args),
            _ => render_generic_call_body(&self.tool_name, &self.args, self.execution_started),
        }
    }

    fn render_result_body(&self) -> Vec<String> {
        let Some(result) = &self.result else {
            return if self.execution_started {
                vec![format!("{DIM}executing...{RESET}")]
            } else {
                Vec::new()
            };
        };

        if result.is_error {
            let output = self.get_text_output();
            if !output.trim().is_empty() {
                return output
                    .lines()
                    .map(|line| format!("{ERROR}{line}{RESET}"))
                    .collect();
            }
        }

        match self.tool_name.as_str() {
            "read" => render_read_result(&self.args, result, self.show_images, self.expanded),
            "write" => render_write_result(result),
            "edit" => render_edit_result(result),
            "bash" => render_bash_result(result, self.show_images, self.expanded),
            "ls" => render_list_result(result, self.show_images, self.expanded),
            "grep" => render_grep_result(result, self.show_images, self.expanded),
            "find" => render_find_result(result, self.show_images, self.expanded),
            _ => render_default_result(result, self.show_images, self.expanded),
        }
    }

    fn get_text_output(&self) -> String {
        let Some(result) = &self.result else {
            return String::new();
        };
        text_output(result, self.show_images)
    }
}

fn text_output(result: &ToolExecutionResult, show_images: bool) -> String {
    let mut parts = Vec::new();
    for block in &result.content {
        match block.r#type.as_str() {
            "text" => {
                if let Some(text) = &block.text {
                    parts.push(text.clone());
                }
            }
            "image" if show_images => {
                let mime = block.mime_type.as_deref().unwrap_or("image");
                let size = block.data.as_ref().map(|data| data.len()).unwrap_or(0);
                parts.push(format!("[image: {mime}, {size} bytes]"));
            }
            _ => {}
        }
    }
    parts.join("\n")
}

fn arg_str(args: &Value, key: &str) -> Option<String> {
    args.get(key).and_then(|v| v.as_str()).map(|s| s.to_string())
}

fn shorten_path(path: &str) -> String {
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

fn format_read_call(args: &Value) -> String {
    let path = shorten_path(arg_str(args, "path").as_deref().unwrap_or(""));
    let offset = args.get("offset").and_then(|v| v.as_u64());
    let limit = args.get("limit").and_then(|v| v.as_u64());
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
    format!("read {ACCENT}{path}{RESET}{MUTED}{line_suffix}{RESET}")
}

fn format_write_call_title(args: &Value) -> String {
    let path = shorten_path(arg_str(args, "path").as_deref().unwrap_or(""));
    format!("write {ACCENT}{path}{RESET}")
}

fn format_edit_call_title(args: &Value) -> String {
    let path = shorten_path(arg_str(args, "path").as_deref().unwrap_or(""));
    format!("edit {ACCENT}{path}{RESET}")
}

fn format_bash_call_title(args: &Value) -> String {
    let command = arg_str(args, "command").unwrap_or_default();
    if command.trim().is_empty() {
        "bash".to_string()
    } else {
        format!("$ {command}")
    }
}

fn format_ls_call(args: &Value) -> String {
    let path = shorten_path(arg_str(args, "path").as_deref().unwrap_or("."));
    if let Some(limit) = args.get("limit").and_then(|v| v.as_u64()) {
        format!("ls {ACCENT}{path}{RESET}{DIM} (limit {limit}){RESET}")
    } else {
        format!("ls {ACCENT}{path}{RESET}")
    }
}

fn format_grep_call(args: &Value) -> String {
    let pattern = arg_str(args, "pattern").unwrap_or_default();
    let path = shorten_path(arg_str(args, "path").as_deref().unwrap_or("."));
    let mut text = format!("grep {ACCENT}/{pattern}/{RESET}{DIM} in {path}{RESET}");
    if let Some(glob) = arg_str(args, "glob") {
        text.push_str(&format!("{DIM} ({glob}){RESET}"));
    }
    text
}

fn format_find_call(args: &Value) -> String {
    let pattern = arg_str(args, "pattern").unwrap_or_default();
    let path = shorten_path(arg_str(args, "path").as_deref().unwrap_or("."));
    format!("find {ACCENT}{pattern}{RESET}{DIM} in {path}{RESET}")
}

fn render_generic_call_body(tool_name: &str, args: &Value, execution_started: bool) -> Vec<String> {
    let mut lines = Vec::new();
    match tool_name {
        "read" | "ls" | "grep" | "find" => {
            let rendered = serde_json::to_string_pretty(args).unwrap_or_else(|_| args.to_string());
            if rendered != "null" && rendered != "{}" {
                lines.extend(rendered.lines().map(|line| format!("{DIM}{line}{RESET}")));
            }
        }
        _ => {
            let rendered = serde_json::to_string_pretty(args).unwrap_or_else(|_| args.to_string());
            if rendered != "null" && rendered != "{}" {
                lines.extend(rendered.lines().map(|line| format!("{DIM}{line}{RESET}")));
            }
        }
    }
    if lines.is_empty() && execution_started {
        lines.push(format!("{DIM}running...{RESET}"));
    }
    lines
}

fn render_bash_call_body(args: &Value) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(timeout) = args.get("timeout").and_then(|v| v.as_f64()) {
        lines.push(format!("{DIM}timeout {timeout}s{RESET}"));
    }
    lines
}

fn render_edit_call_body(args: &Value) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(edits) = args.get("edits").and_then(|v| v.as_array()) {
        lines.push(format!("{DIM}{} edit block(s){RESET}", edits.len()));
        for (index, edit) in edits.iter().take(3).enumerate() {
            let old_text = edit.get("oldText").and_then(|v| v.as_str()).unwrap_or("");
            let new_text = edit.get("newText").and_then(|v| v.as_str()).unwrap_or("");
            let old_preview = summarize_inline(old_text, 60);
            let new_preview = summarize_inline(new_text, 60);
            lines.push(format!("{DIM}{}.{RESET} - {old_preview}", index + 1));
            lines.push(format!("{DIM}   +{RESET} {new_preview}"));
        }
        if edits.len() > 3 {
            lines.push(format!("{DIM}... ({} more edit block(s)){RESET}", edits.len() - 3));
        }
    }
    lines
}

fn render_write_call_body(args: &Value, expanded: bool) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(content) = arg_str(args, "content") {
        let preview_lines: Vec<String> = content.lines().map(|line| line.to_string()).collect();
        let max_lines = if expanded { 10 } else { 5 };
        lines.extend(
            preview_lines
                .iter()
                .take(max_lines)
                .map(|line| format!("{DIM}{}{RESET}", replace_tabs(line))),
        );
        if preview_lines.len() > max_lines {
            lines.push(format!("{DIM}... ({} more lines){RESET}", preview_lines.len() - max_lines));
        }
    }
    lines
}

fn render_read_result(args: &Value, result: &ToolExecutionResult, show_images: bool, expanded: bool) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(details) = &result.details {
        let path = details
            .get("path")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| arg_str(args, "path"))
            .unwrap_or_default();
        let start = details.get("startLine").and_then(|v| v.as_u64()).unwrap_or(1);
        let end = details.get("endLine").and_then(|v| v.as_u64()).unwrap_or(start);
        let total = details.get("totalLines").and_then(|v| v.as_u64()).unwrap_or(end);
        if !path.is_empty() {
            lines.push(format!("{DIM}read {} lines {start}-{end} / {total}{RESET}", shorten_path(&path)));
        }
    }
    lines.extend(preview_text_lines(&text_output(result, show_images), if expanded { 120 } else { 10 }));
    lines
}

fn render_write_result(result: &ToolExecutionResult) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(details) = &result.details {
        let path = details.get("path").and_then(|v| v.as_str()).unwrap_or("");
        let bytes = details.get("bytes").and_then(|v| v.as_u64()).unwrap_or(0);
        lines.push(format!("{DIM}wrote {bytes} bytes to {}{RESET}", shorten_path(path)));
    }
    lines
}

fn render_edit_result(result: &ToolExecutionResult) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(details) = &result.details {
        let path = details.get("path").and_then(|v| v.as_str()).unwrap_or("");
        let applied = details.get("applied").and_then(|v| v.as_u64()).unwrap_or(0);
        let total = details.get("total").and_then(|v| v.as_u64()).unwrap_or(0);
        lines.push(format!("{DIM}applied {applied}/{total} edit(s) to {}{RESET}", shorten_path(path)));
        if let Some(diff) = details.get("diff").and_then(|v| v.as_str()) {
            lines.extend(render_diff_lines(diff));
            return lines;
        }
    }
    lines.extend(preview_text_lines(&text_output(result, false), 80));
    lines
}

fn render_bash_result(result: &ToolExecutionResult, show_images: bool, expanded: bool) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(details) = &result.details {
        let exit = details.get("exitCode").and_then(|v| v.as_i64()).unwrap_or(-1);
        let truncated = details.get("truncated").and_then(|v| v.as_bool()).unwrap_or(false);
        let cancelled = details.get("cancelled").and_then(|v| v.as_bool()).unwrap_or(false);
        let mut flags = Vec::new();
        if truncated {
            flags.push("truncated");
        }
        if cancelled {
            flags.push("cancelled");
        }
        let suffix = if flags.is_empty() {
            String::new()
        } else {
            format!(" [{}]", flags.join(", "))
        };
        lines.push(format!("{DIM}exit code: {exit}{suffix}{RESET}"));
    }
    lines.extend(preview_text_lines(&text_output(result, show_images), if expanded { 120 } else { 12 }));
    lines
}

fn render_list_result(result: &ToolExecutionResult, show_images: bool, expanded: bool) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(details) = &result.details {
        let count = details.get("entryCount").and_then(|v| v.as_u64()).unwrap_or(0);
        let truncated = details.get("truncated").and_then(|v| v.as_bool()).unwrap_or(false);
        let suffix = if truncated { " (truncated)" } else { "" };
        lines.push(format!("{DIM}{count} entr{} shown{suffix}{RESET}", if count == 1 { "y" } else { "ies" }));
    }
    lines.extend(preview_text_lines(&text_output(result, show_images), if expanded { 120 } else { 20 }));
    lines
}

fn render_grep_result(result: &ToolExecutionResult, show_images: bool, expanded: bool) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(details) = &result.details {
        let count = details.get("matchCount").and_then(|v| v.as_u64()).unwrap_or(0);
        lines.push(format!("{DIM}{count} match(es){RESET}"));
    }
    lines.extend(preview_text_lines(&text_output(result, show_images), if expanded { 120 } else { 15 }));
    lines
}

fn render_find_result(result: &ToolExecutionResult, show_images: bool, expanded: bool) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(details) = &result.details {
        let count = details.get("matchCount").and_then(|v| v.as_u64()).unwrap_or(0);
        lines.push(format!("{DIM}{count} file(s){RESET}"));
    }
    lines.extend(preview_text_lines(&text_output(result, show_images), if expanded { 120 } else { 20 }));
    lines
}

fn render_default_result(result: &ToolExecutionResult, show_images: bool, expanded: bool) -> Vec<String> {
    preview_text_lines(&text_output(result, show_images), if expanded { 120 } else { 20 })
}

fn preview_text_lines(text: &str, max_lines: usize) -> Vec<String> {
    if text.trim().is_empty() {
        return Vec::new();
    }

    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    for line in lines.iter().take(max_lines) {
        out.push(replace_tabs(line));
    }
    if lines.len() > max_lines {
        out.push(format!("{DIM}... ({} more lines){RESET}", lines.len() - max_lines));
    }
    out
}

fn summarize_inline(text: &str, max_chars: usize) -> String {
    let text = text.replace('\n', "\\n");
    if text.chars().count() <= max_chars {
        text
    } else {
        let prefix: String = text.chars().take(max_chars.saturating_sub(3)).collect();
        format!("{prefix}...")
    }
}

fn replace_tabs(text: &str) -> String {
    text.replace('\t', "   ")
}

fn render_box_lines(content: &[String], width: u16, bg: &str) -> Vec<String> {
    let total_width = width.max(4) as usize;
    let inner_width = total_width.saturating_sub(2).max(1);
    let mut lines = vec![String::new()];
    lines.push(apply_bg_line("", total_width, bg));

    for line in content {
        let wrapped = wrap_ansi_line(line, inner_width);
        if wrapped.is_empty() {
            lines.push(apply_bg_line("", total_width, bg));
        } else {
            for wrapped_line in wrapped {
                lines.push(apply_bg_line(&wrapped_line, total_width, bg));
            }
        }
    }

    lines.push(apply_bg_line("", total_width, bg));
    lines
}

fn apply_bg_line(content: &str, total_width: usize, bg: &str) -> String {
    let visible = visible_width(content);
    let inner_width = total_width.saturating_sub(2);
    let pad = inner_width.saturating_sub(visible);
    format!("{bg} {}{} {RESET}", content, " ".repeat(pad))
}

fn wrap_ansi_line(line: &str, width: usize) -> Vec<String> {
    if line.is_empty() {
        return vec![String::new()];
    }
    if visible_width(line) <= width {
        return vec![line.to_string()];
    }

    let mut out = Vec::new();
    let mut current = String::new();

    for word in line.split(' ') {
        let candidate = if current.is_empty() {
            word.to_string()
        } else {
            format!("{current} {word}")
        };
        if visible_width(&candidate) <= width {
            current = candidate;
            continue;
        }
        if !current.is_empty() {
            out.push(current);
            current = String::new();
        }
        if visible_width(word) <= width {
            current = word.to_string();
        } else {
            let mut chunk = String::new();
            for ch in word.chars() {
                let next = format!("{chunk}{ch}");
                if visible_width(&next) > width && !chunk.is_empty() {
                    out.push(chunk);
                    chunk = String::new();
                }
                chunk.push(ch);
            }
            if !chunk.is_empty() {
                current = chunk;
            }
        }
    }

    if !current.is_empty() {
        out.push(current);
    }

    if out.is_empty() {
        vec![line.to_string()]
    } else {
        out
    }
}
