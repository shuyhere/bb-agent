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
        let dim = "\x1b[90m";
        let green = "\x1b[32m";
        let red = "\x1b[31m";
        let reset = "\x1b[0m";
        let bold = "\x1b[1m";

        // Status indicator
        let status = if self.result.is_some() {
            if self.result.as_ref().map(|r| r.is_error).unwrap_or(false) {
                format!("{red}x{reset}")
            } else {
                format!("{green}*{reset}")
            }
        } else if self.execution_started {
            format!("{dim}..{reset}")
        } else {
            format!("{dim}>{reset}")
        };

        // One-line summary: status tool_name(args_preview)
        let args_preview = if self.args.is_null() || self.args.is_object() && self.args.as_object().map(|o| o.is_empty()).unwrap_or(true) {
            String::new()
        } else {
            let s = self.args.to_string();
            if s.len() > 80 { format!("({}...)", &s[..77]) } else { format!("({s})") }
        };

        let header = format!("{status} {bold}{}{reset}{dim}{args_preview}{reset}", self.tool_name);

        if !self.expanded {
            // Collapsed: just the header line
            return vec![header];
        }

        // Expanded: header + output
        let mut lines = vec![header];
        let output = self.get_text_output();
        if !output.is_empty() {
            // Truncate very long output
            let output_lines: Vec<&str> = output.lines().collect();
            let max_lines = 30;
            for line in output_lines.iter().take(max_lines) {
                lines.push(format!("{dim}  {line}{reset}"));
            }
            if output_lines.len() > max_lines {
                lines.push(format!("{dim}  ... ({} more lines){reset}", output_lines.len() - max_lines));
            }
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

        if self.expanded {
            parts.join("\n")
        } else {
            truncate_preview(&parts.join("\n"), 20)
        }
    }
}

fn truncate_preview(text: &str, max_lines: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() <= max_lines {
        return text.to_string();
    }

    let start = lines.len().saturating_sub(max_lines);
    let mut preview = String::from("...\n");
    preview.push_str(&lines[start..].join("\n"));
    preview
}
