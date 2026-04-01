use bb_core::types::*;
use crossterm::style::{Attribute, Color, Stylize};

/// Render a message for terminal display.
pub fn render_message(msg: &AgentMessage) -> Vec<String> {
    match msg {
        AgentMessage::User(u) => render_user(u),
        AgentMessage::Assistant(a) => render_assistant(a),
        AgentMessage::ToolResult(t) => render_tool_result(t),
        AgentMessage::BashExecution(b) => render_bash(b),
        AgentMessage::CompactionSummary(c) => render_compaction(c),
        AgentMessage::BranchSummary(b) => render_branch_summary(b),
        AgentMessage::Custom(c) => render_custom(c),
    }
}

fn render_user(msg: &UserMessage) -> Vec<String> {
    let mut lines = vec![format!("{}", "You".bold().with(Color::Blue))];
    for block in &msg.content {
        match block {
            ContentBlock::Text { text } => {
                lines.extend(text.lines().map(|l| format!("  {l}")));
            }
            ContentBlock::Image { .. } => {
                lines.push("  [image attached]".to_string());
            }
        }
    }
    lines.push(String::new());
    lines
}

fn render_assistant(msg: &AssistantMessage) -> Vec<String> {
    let model_tag = format!(" ({})", msg.model);
    let mut lines = vec![format!(
        "{}{}",
        "Assistant".bold().with(Color::Green),
        model_tag.with(Color::DarkGrey),
    )];

    for block in &msg.content {
        match block {
            AssistantContent::Text { text } => {
                lines.extend(text.lines().map(|l| format!("  {l}")));
            }
            AssistantContent::Thinking { thinking } => {
                lines.push(format!(
                    "  {}",
                    "[thinking]".with(Color::DarkGrey).attribute(Attribute::Dim)
                ));
                for l in thinking.lines().take(3) {
                    lines.push(format!(
                        "  {}",
                        l.with(Color::DarkGrey).attribute(Attribute::Dim)
                    ));
                }
                let total = thinking.lines().count();
                if total > 3 {
                    lines.push(format!(
                        "  {}",
                        format!("[{} more lines]", total - 3)
                            .with(Color::DarkGrey)
                            .attribute(Attribute::Dim)
                    ));
                }
            }
            AssistantContent::ToolCall { name, arguments, .. } => {
                let args_str = serde_json::to_string(arguments).unwrap_or_default();
                let preview = if args_str.len() > 100 {
                    format!("{}...", &args_str[..100])
                } else {
                    args_str
                };
                lines.push(format!(
                    "  {} {}({})",
                    "*".with(Color::Yellow),
                    name.clone().bold(),
                    preview.with(Color::DarkGrey),
                ));
            }
        }
    }
    lines.push(String::new());
    lines
}

fn render_tool_result(msg: &ToolResultMessage) -> Vec<String> {
    let status = if msg.is_error {
        "✗".with(Color::Red).to_string()
    } else {
        "✓".with(Color::Green).to_string()
    };

    let mut lines = vec![format!(
        "  {} {} result:",
        status,
        msg.tool_name.clone().with(Color::Cyan),
    )];

    for block in &msg.content {
        if let ContentBlock::Text { text } = block {
            let preview_lines: Vec<&str> = text.lines().take(10).collect();
            for l in &preview_lines {
                lines.push(format!("    {l}"));
            }
            let total = text.lines().count();
            if total > 10 {
                lines.push(format!(
                    "    {}",
                    format!("[{} more lines]", total - 10).with(Color::DarkGrey)
                ));
            }
        }
    }
    lines.push(String::new());
    lines
}

fn render_bash(msg: &BashExecutionMessage) -> Vec<String> {
    let mut lines = vec![format!(
        "  {} $ {}",
        "*".with(Color::Yellow),
        msg.command.clone().with(Color::Cyan),
    )];
    let preview: Vec<&str> = msg.output.lines().take(10).collect();
    for l in &preview {
        lines.push(format!("    {l}"));
    }
    lines.push(String::new());
    lines
}

fn render_compaction(msg: &CompactionSummaryMessage) -> Vec<String> {
    vec![
        format!(
            "{} [compaction: {} tokens summarized]",
            "[c]".with(Color::DarkGrey),
            msg.tokens_before,
        ),
        String::new(),
    ]
}

fn render_branch_summary(msg: &BranchSummaryMessage) -> Vec<String> {
    vec![
        format!(
            "{} [branch summary from {}]",
            "[b]".with(Color::DarkGrey),
            msg.from_id,
        ),
        String::new(),
    ]
}

fn render_custom(msg: &CustomMessage) -> Vec<String> {
    if !msg.display {
        return Vec::new();
    }
    let mut lines = vec![format!(
        "{} [{}]",
        "[+]".with(Color::DarkGrey),
        msg.custom_type,
    )];
    for block in &msg.content {
        if let ContentBlock::Text { text } = block {
            lines.extend(text.lines().map(|l| format!("  {l}")));
        }
    }
    lines.push(String::new());
    lines
}
