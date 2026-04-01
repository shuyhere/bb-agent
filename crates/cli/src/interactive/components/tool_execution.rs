use serde_json::Value;

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

    pub fn render_lines(&self) -> Vec<String> {
        let reset = "\x1b[0m";
        let dim = "\x1b[90m";
        let bold = "\x1b[1m";
        let accent = "\x1b[38;2;178;148;187m";
        let success = "\x1b[32m";
        let error = "\x1b[31m";

        let (status_mark, border_color) = if let Some(result) = &self.result {
            if result.is_error {
                (format!("{error}x{reset}"), error)
            } else {
                (format!("{success}*{reset}"), success)
            }
        } else if self.execution_started {
            (format!("{dim}..{reset}"), accent)
        } else {
            (format!("{dim}>{reset}"), accent)
        };

        let title = self.render_call_title();
        if !self.expanded {
            return vec![format!("{status_mark} {bold}{title}{reset}")];
        }

        let mut lines = vec![String::new()];
        lines.push(format!("{border_color}╭─ {status_mark} {bold}{title}{reset}"));

        for line in self.render_call_body() {
            lines.push(format!("{border_color}│{reset} {line}"));
        }

        let result_lines = self.render_result_body();
        if !result_lines.is_empty() {
            lines.push(format!("{border_color}├{reset}"));
            for line in result_lines {
                lines.push(format!("{border_color}│{reset} {line}"));
            }
        }

        lines.push(format!("{border_color}╰{reset}"));
        lines
    }

    fn render_call_title(&self) -> String {
        match self.tool_name.as_str() {
            "read" => format!("Read {}", shorten_path(arg_str(&self.args, "path").as_deref().unwrap_or(""))),
            "write" => format!("Write {}", shorten_path(arg_str(&self.args, "path").as_deref().unwrap_or(""))),
            "edit" => format!("Edit {}", shorten_path(arg_str(&self.args, "path").as_deref().unwrap_or(""))),
            "bash" => "Bash".to_string(),
            "ls" => format!("Ls {}", shorten_path(arg_str(&self.args, "path").as_deref().unwrap_or("."))),
            "grep" => format!(
                "Grep {}",
                arg_str(&self.args, "pattern").unwrap_or_default()
            ),
            "find" => format!(
                "Find {}",
                arg_str(&self.args, "pattern").unwrap_or_default()
            ),
            other => other.to_string(),
        }
    }

    fn render_call_body(&self) -> Vec<String> {
        let dim = "\x1b[90m";
        let reset = "\x1b[0m";
        let mut lines = Vec::new();

        match self.tool_name.as_str() {
            "bash" => {
                if let Some(command) = arg_str(&self.args, "command") {
                    lines.push(format!("{dim}$ {command}{reset}"));
                }
                if let Some(timeout) = self.args.get("timeout").and_then(|v| v.as_f64()) {
                    lines.push(format!("{dim}timeout: {timeout}s{reset}"));
                }
            }
            "read" => {
                if let Some(path) = arg_str(&self.args, "path") {
                    lines.push(format!("{dim}path: {}{reset}", shorten_path(&path)));
                }
                if let Some(offset) = self.args.get("offset").and_then(|v| v.as_u64()) {
                    lines.push(format!("{dim}offset: {offset}{reset}"));
                }
                if let Some(limit) = self.args.get("limit").and_then(|v| v.as_u64()) {
                    lines.push(format!("{dim}limit: {limit}{reset}"));
                }
            }
            "write" => {
                if let Some(path) = arg_str(&self.args, "path") {
                    lines.push(format!("{dim}path: {}{reset}", shorten_path(&path)));
                }
                if let Some(content) = arg_str(&self.args, "content") {
                    lines.push(format!("{dim}bytes: {}{reset}", content.len()));
                }
            }
            "edit" => {
                if let Some(path) = arg_str(&self.args, "path") {
                    lines.push(format!("{dim}path: {}{reset}", shorten_path(&path)));
                }
                if let Some(edits) = self.args.get("edits").and_then(|v| v.as_array()) {
                    lines.push(format!("{dim}edits: {}{reset}", edits.len()));
                }
            }
            "ls" => {
                if let Some(path) = arg_str(&self.args, "path") {
                    lines.push(format!("{dim}path: {}{reset}", shorten_path(&path)));
                }
                if let Some(limit) = self.args.get("limit").and_then(|v| v.as_u64()) {
                    lines.push(format!("{dim}limit: {limit}{reset}"));
                }
            }
            "grep" => {
                if let Some(pattern) = arg_str(&self.args, "pattern") {
                    lines.push(format!("{dim}pattern: {pattern}{reset}"));
                }
                if let Some(path) = arg_str(&self.args, "path") {
                    lines.push(format!("{dim}path: {}{reset}", shorten_path(&path)));
                }
                if let Some(glob) = arg_str(&self.args, "glob") {
                    lines.push(format!("{dim}glob: {glob}{reset}"));
                }
            }
            "find" => {
                if let Some(pattern) = arg_str(&self.args, "pattern") {
                    lines.push(format!("{dim}pattern: {pattern}{reset}"));
                }
                if let Some(path) = arg_str(&self.args, "path") {
                    lines.push(format!("{dim}path: {}{reset}", shorten_path(&path)));
                }
            }
            _ => {
                if !self.args.is_null() {
                    let rendered = serde_json::to_string_pretty(&self.args)
                        .unwrap_or_else(|_| self.args.to_string());
                    lines.extend(rendered.lines().map(|l| format!("{dim}{l}{reset}")));
                }
            }
        }

        if lines.is_empty() {
            if self.execution_started {
                lines.push(format!("{dim}running...{reset}"));
            }
        }
        lines
    }

    fn render_result_body(&self) -> Vec<String> {
        let Some(result) = &self.result else {
            return if self.execution_started {
                vec!["\x1b[90mexecuting...\x1b[0m".to_string()]
            } else {
                Vec::new()
            };
        };

        let dim = "\x1b[90m";
        let reset = "\x1b[0m";
        let mut lines = Vec::new();

        if let Some(details) = &result.details {
            lines.extend(render_detail_summary(&self.tool_name, details));
        }

        let output = self.get_text_output();
        if !output.is_empty() {
            let max_lines = 80;
            let output_lines: Vec<&str> = output.lines().collect();
            for line in output_lines.iter().take(max_lines) {
                lines.push((*line).to_string());
            }
            if output_lines.len() > max_lines {
                lines.push(format!("{dim}... ({} more lines){reset}", output_lines.len() - max_lines));
            }
        }

        if lines.is_empty() && result.is_error {
            lines.push("error".to_string());
        }
        lines
    }

    fn get_text_output(&self) -> String {
        let Some(result) = &self.result else {
            return String::new();
        };

        let mut parts = Vec::new();
        for block in &result.content {
            match block.r#type.as_str() {
                "text" => {
                    if let Some(text) = &block.text {
                        parts.push(text.clone());
                    }
                }
                "image" if self.show_images => {
                    let mime = block.mime_type.as_deref().unwrap_or("image");
                    let size = block.data.as_ref().map(|data| data.len()).unwrap_or(0);
                    parts.push(format!("[image: {mime}, {size} bytes]"));
                }
                _ => {}
            }
        }
        parts.join("\n")
    }
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

fn render_detail_summary(tool_name: &str, details: &Value) -> Vec<String> {
    let dim = "\x1b[90m";
    let reset = "\x1b[0m";
    let mut lines = Vec::new();
    match tool_name {
        "read" => {
            let path = details.get("path").and_then(|v| v.as_str()).unwrap_or("");
            let start = details.get("startLine").and_then(|v| v.as_u64()).unwrap_or(0);
            let end = details.get("endLine").and_then(|v| v.as_u64()).unwrap_or(0);
            let total = details.get("totalLines").and_then(|v| v.as_u64()).unwrap_or(0);
            if !path.is_empty() {
                lines.push(format!("{dim}read {} lines {start}-{end} / {total}{reset}", shorten_path(path)));
            }
        }
        "write" => {
            let path = details.get("path").and_then(|v| v.as_str()).unwrap_or("");
            let bytes = details.get("bytes").and_then(|v| v.as_u64()).unwrap_or(0);
            lines.push(format!("{dim}wrote {bytes} bytes to {}{reset}", shorten_path(path)));
        }
        "edit" => {
            let path = details.get("path").and_then(|v| v.as_str()).unwrap_or("");
            let applied = details.get("applied").and_then(|v| v.as_u64()).unwrap_or(0);
            let total = details.get("total").and_then(|v| v.as_u64()).unwrap_or(0);
            lines.push(format!("{dim}applied {applied}/{total} edit(s) to {}{reset}", shorten_path(path)));
        }
        "bash" => {
            let exit = details.get("exitCode").and_then(|v| v.as_i64()).unwrap_or(-1);
            let truncated = details.get("truncated").and_then(|v| v.as_bool()).unwrap_or(false);
            let cancelled = details.get("cancelled").and_then(|v| v.as_bool()).unwrap_or(false);
            let mut extra = Vec::new();
            if truncated {
                extra.push("truncated");
            }
            if cancelled {
                extra.push("cancelled");
            }
            let suffix = if extra.is_empty() {
                String::new()
            } else {
                format!(" [{}]", extra.join(", "))
            };
            lines.push(format!("{dim}exit code: {exit}{suffix}{reset}"));
        }
        "grep" => {
            let count = details.get("matchCount").and_then(|v| v.as_u64()).unwrap_or(0);
            let truncated = details.get("truncated").and_then(|v| v.as_bool()).unwrap_or(false);
            let suffix = if truncated { " (truncated)" } else { "" };
            lines.push(format!("{dim}{count} match(es){suffix}{reset}"));
        }
        "find" => {
            let count = details.get("matchCount").and_then(|v| v.as_u64()).unwrap_or(0);
            let truncated = details.get("truncated").and_then(|v| v.as_bool()).unwrap_or(false);
            let suffix = if truncated { " (truncated)" } else { "" };
            lines.push(format!("{dim}{count} file(s){suffix}{reset}"));
        }
        "ls" => {
            let count = details.get("entryCount").and_then(|v| v.as_u64()).unwrap_or(0);
            let truncated = details.get("truncated").and_then(|v| v.as_bool()).unwrap_or(false);
            let suffix = if truncated { " (truncated)" } else { "" };
            lines.push(format!("{dim}{count} entr{} shown{suffix}{reset}", if count == 1 { "y" } else { "ies" }));
        }
        _ => {}
    }
    lines
}
