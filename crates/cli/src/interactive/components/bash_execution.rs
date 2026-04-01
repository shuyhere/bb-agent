const PREVIEW_LINES: usize = 20;
const DEFAULT_MAX_LINES: usize = 2_000;
const DEFAULT_MAX_BYTES: usize = 50 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BashStatus {
    Running,
    Complete,
    Cancelled,
    Error,
}

#[derive(Debug, Clone, Default)]
pub struct TruncationResult {
    pub content: String,
    pub truncated: bool,
    pub total_lines: usize,
    pub total_bytes: usize,
}

#[derive(Debug, Clone)]
pub struct BashExecutionComponent {
    command: String,
    output_lines: Vec<String>,
    status: BashStatus,
    exit_code: Option<i32>,
    truncation_result: Option<TruncationResult>,
    full_output_path: Option<String>,
    expanded: bool,
    exclude_from_context: bool,
}

impl BashExecutionComponent {
    pub fn new(command: impl Into<String>, exclude_from_context: bool) -> Self {
        Self {
            command: command.into(),
            output_lines: Vec::new(),
            status: BashStatus::Running,
            exit_code: None,
            truncation_result: None,
            full_output_path: None,
            expanded: false,
            exclude_from_context,
        }
    }

    pub fn set_expanded(&mut self, expanded: bool) {
        self.expanded = expanded;
    }

    pub fn append_output(&mut self, chunk: &str) {
        let clean = strip_ansi(chunk).replace("\r\n", "\n").replace('\r', "\n");
        let new_lines: Vec<String> = clean.split('\n').map(ToString::to_string).collect();

        if !self.output_lines.is_empty() && !new_lines.is_empty() {
            if let Some(last) = self.output_lines.last_mut() {
                last.push_str(&new_lines[0]);
            }
            self.output_lines.extend(new_lines.into_iter().skip(1));
        } else {
            self.output_lines.extend(new_lines);
        }
    }

    pub fn set_complete(
        &mut self,
        exit_code: Option<i32>,
        cancelled: bool,
        truncation_result: Option<TruncationResult>,
        full_output_path: Option<String>,
    ) {
        self.exit_code = exit_code;
        self.status = if cancelled {
            BashStatus::Cancelled
        } else if matches!(exit_code, Some(code) if code != 0) {
            BashStatus::Error
        } else {
            BashStatus::Complete
        };
        self.truncation_result = truncation_result;
        self.full_output_path = full_output_path;
    }

    pub fn render_lines(&self) -> Vec<String> {
        let full_output = self.output_lines.join("\n");
        let context_truncation = truncate_tail(&full_output, DEFAULT_MAX_LINES, DEFAULT_MAX_BYTES);
        let available_lines: Vec<&str> = if context_truncation.content.is_empty() {
            Vec::new()
        } else {
            context_truncation.content.lines().collect()
        };

        let preview_start = available_lines.len().saturating_sub(PREVIEW_LINES);
        let preview_lines = &available_lines[preview_start..];
        let hidden_line_count = available_lines.len().saturating_sub(preview_lines.len());

        let mut lines = Vec::new();
        let prefix = if self.exclude_from_context { "!!" } else { "$" };
        lines.push(format!("{prefix} {}", self.command));

        if !available_lines.is_empty() {
            lines.push(String::new());
            let body = if self.expanded {
                available_lines.to_vec()
            } else {
                preview_lines.to_vec()
            };
            lines.extend(body.into_iter().map(|line| line.to_string()));
        }

        match self.status {
            BashStatus::Running => lines.push(String::from("Running...")),
            BashStatus::Cancelled => lines.push(String::from("(cancelled)")),
            BashStatus::Error => lines.push(format!("(exit {})", self.exit_code.unwrap_or(-1))),
            BashStatus::Complete => {}
        }

        if hidden_line_count > 0 {
            if self.expanded {
                lines.push(String::from("(collapse to hide output)"));
            } else {
                lines.push(format!(
                    "... {hidden_line_count} more lines (expand to view)"
                ));
            }
        }

        let was_truncated = self
            .truncation_result
            .as_ref()
            .map(|result| result.truncated)
            .unwrap_or(false)
            || context_truncation.truncated;
        if was_truncated {
            if let Some(path) = &self.full_output_path {
                lines.push(format!("Output truncated. Full output: {path}"));
            }
        }

        lines
    }

    pub fn get_output(&self) -> String {
        self.output_lines.join("\n")
    }

    pub fn get_command(&self) -> &str {
        &self.command
    }

    pub fn status(&self) -> BashStatus {
        self.status
    }
}

pub fn truncate_tail(content: &str, max_lines: usize, max_bytes: usize) -> TruncationResult {
    let total_lines = content.lines().count();
    let total_bytes = content.len();

    if total_lines <= max_lines && total_bytes <= max_bytes {
        return TruncationResult {
            content: content.to_string(),
            truncated: false,
            total_lines,
            total_bytes,
        };
    }

    let mut lines: Vec<&str> = content.lines().collect();
    if lines.len() > max_lines {
        lines = lines.split_off(lines.len() - max_lines);
    }

    let mut joined = lines.join("\n");
    while joined.len() > max_bytes {
        if let Some(pos) = joined.find('\n') {
            joined = joined[pos + 1..].to_string();
        } else {
            let start = joined.len().saturating_sub(max_bytes);
            joined = joined[start..].to_string();
            break;
        }
    }

    TruncationResult {
        content: joined,
        truncated: true,
        total_lines,
        total_bytes,
    }
}

fn strip_ansi(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch != '\u{1b}' {
            output.push(ch);
            continue;
        }

        if chars.peek() == Some(&'[') {
            chars.next();
            while let Some(next) = chars.next() {
                if ('@'..='~').contains(&next) {
                    break;
                }
            }
        }
    }

    output
}
