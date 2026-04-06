use std::collections::HashMap;

use bb_session::tree::TreeNode;

use super::FlatNode;

#[derive(Clone, Debug)]
pub(super) struct VisibleNode {
    entry_id: String,
    entry_type: String,
    preview: String,
    is_active: bool,
    children: Vec<VisibleNode>,
}

pub(super) fn extract_preview(node: &TreeNode) -> String {
    match node.entry_type.as_str() {
        "message" => format!("message: {}", &node.entry_id[..8.min(node.entry_id.len())]),
        "compaction" => "[compaction]".to_string(),
        "branch_summary" => "[branch summary]".to_string(),
        "model_change" => "[model change]".to_string(),
        "thinking_level_change" => "[thinking level change]".to_string(),
        "session_info" => "[session info]".to_string(),
        other => format!("[{other}]"),
    }
}

fn is_default_tree_node_visible(entry_type: &str, preview: &str) -> bool {
    match entry_type {
        "message" => !preview.starts_with("[tool result:") && !preview.starts_with("[bash:"),
        "compaction" | "branch_summary" => true,
        _ => false,
    }
}

pub(super) fn build_visible_nodes(
    nodes: &[TreeNode],
    payloads: &HashMap<&str, &str>,
    active_leaf: Option<&str>,
) -> Vec<VisibleNode> {
    let mut out = Vec::new();
    for node in nodes {
        let preview = payloads
            .get(node.entry_id.as_str())
            .map(|payload| extract_preview_from_payload(&node.entry_type, payload))
            .unwrap_or_else(|| extract_preview(node));
        let children = build_visible_nodes(&node.children, payloads, active_leaf);
        let visible = is_default_tree_node_visible(&node.entry_type, &preview);
        if visible {
            out.push(VisibleNode {
                entry_id: node.entry_id.clone(),
                entry_type: node.entry_type.clone(),
                preview,
                is_active: active_leaf == Some(node.entry_id.as_str()),
                children,
            });
        } else {
            out.extend(children);
        }
    }
    out
}

pub(super) fn flatten_visible(
    nodes: &[VisibleNode],
    depth: usize,
    parent_id: Option<&str>,
    ancestor_is_last: &[bool],
    ancestor_ids: &[String],
) -> Vec<FlatNode> {
    let mut flat = Vec::new();
    let count = nodes.len();
    for (i, node) in nodes.iter().enumerate() {
        let is_last = i == count - 1;
        flat.push(FlatNode {
            entry_id: node.entry_id.clone(),
            parent_id: parent_id.map(str::to_string),
            entry_type: node.entry_type.clone(),
            depth,
            preview: node.preview.clone(),
            is_active: node.is_active,
            has_children: !node.children.is_empty(),
            is_last_child: is_last,
            ancestor_is_last: ancestor_is_last.to_vec(),
            ancestor_ids: ancestor_ids.to_vec(),
        });

        let child_depth = if node.children.len() > 1 {
            depth + 1
        } else {
            depth
        };
        let mut child_ancestor_is_last = ancestor_is_last.to_vec();
        if child_depth > depth {
            child_ancestor_is_last.push(is_last);
        }
        let mut child_ancestor_ids = ancestor_ids.to_vec();
        child_ancestor_ids.push(node.entry_id.clone());
        flat.extend(flatten_visible(
            &node.children,
            child_depth,
            Some(node.entry_id.as_str()),
            &child_ancestor_is_last,
            &child_ancestor_ids,
        ));
    }
    flat
}

fn format_tool_call_preview(name: &str, arguments: Option<&serde_json::Value>) -> String {
    let name_title = match name {
        "bash" => "Bash",
        "read" => "Read",
        "write" => "Write",
        "edit" => "Edit",
        "grep" => "Grep",
        "find" => "Find",
        "ls" => "LS",
        other => other,
    };
    let Some(arguments) = arguments else {
        return name_title.to_string();
    };
    let field = match name {
        "bash" => "command",
        "read" | "write" | "edit" => "path",
        "grep" => "pattern",
        _ => "",
    };
    if field.is_empty() {
        return name_title.to_string();
    }
    let value = arguments
        .get(field)
        .and_then(|v| v.as_str())
        .map(|s| truncate_str(s, 32))
        .unwrap_or_default();
    if value.is_empty() {
        name_title.to_string()
    } else {
        format!("{name_title}({value})")
    }
}

pub(super) fn extract_preview_from_payload(entry_type: &str, payload: &str) -> String {
    let parsed: serde_json::Value = match serde_json::from_str(payload) {
        Ok(v) => v,
        Err(_) => return format!("[{entry_type}]"),
    };

    match entry_type {
        "message" => {
            let Some(msg) = parsed.get("message") else {
                return "[message]".to_string();
            };

            let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or_else(|| {
                if msg.get("tool_call_id").is_some() {
                    "toolResult"
                } else if msg.get("command").is_some() {
                    "bashExecution"
                } else if msg.get("provider").is_some() {
                    "assistant"
                } else {
                    "user"
                }
            });
            match role {
                "user" => {
                    if let Some(arr) = msg.get("content").and_then(|c| c.as_array()) {
                        for block in arr {
                            if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                                let truncated = truncate_str(text, 60);
                                return format!("user: \"{truncated}\"");
                            }
                        }
                    }
                    "user".to_string()
                }
                "assistant" => {
                    if let Some(arr) = msg.get("content").and_then(|c| c.as_array()) {
                        let mut tool_calls = Vec::new();
                        let mut thinking_preview: Option<String> = None;
                        for block in arr {
                            if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                                let truncated = truncate_str(text, 40);
                                return format!("assistant: \"{truncated}\"");
                            }
                            if block.get("type").and_then(|t| t.as_str()) == Some("thinking")
                                && let Some(thinking) =
                                    block.get("thinking").and_then(|t| t.as_str())
                            {
                                let truncated = truncate_str(thinking, 40);
                                thinking_preview = Some(format!("think: \"{truncated}\""));
                            }
                            if block.get("type").and_then(|t| t.as_str()) == Some("toolCall") {
                                let name =
                                    block.get("name").and_then(|n| n.as_str()).unwrap_or("tool");
                                let rendered =
                                    format_tool_call_preview(name, block.get("arguments"));
                                tool_calls.push(rendered);
                            }
                        }
                        if !tool_calls.is_empty() {
                            return tool_calls
                                .into_iter()
                                .take(2)
                                .collect::<Vec<_>>()
                                .join(", ");
                        }
                        if let Some(thinking_preview) = thinking_preview {
                            return thinking_preview;
                        }
                    }
                    "assistant".to_string()
                }
                "toolResult" => {
                    let name = msg
                        .get("tool_name")
                        .and_then(|n| n.as_str())
                        .unwrap_or("tool");
                    format!("[tool result: {name}]")
                }
                "bashExecution" => {
                    let cmd = msg
                        .get("command")
                        .and_then(|c| c.as_str())
                        .map(|c| truncate_str(c, 40))
                        .unwrap_or_else(|| "bash".to_string());
                    format!("[bash: {cmd}]")
                }
                other => format!("[{other}]"),
            }
        }
        "compaction" => {
            let tokens = parsed
                .get("tokens_before")
                .and_then(|t| t.as_u64())
                .map(|t| format!("{t}"))
                .unwrap_or_else(|| "?".to_string());
            format!("[compaction: {tokens} tokens]")
        }
        "branch_summary" => "[branch summary]".to_string(),
        "model_change" => {
            let model = parsed
                .get("model_id")
                .and_then(|m| m.as_str())
                .unwrap_or("unknown");
            format!("[model: {model}]")
        }
        "thinking_level_change" => {
            let level = parsed
                .get("thinking_level")
                .and_then(|l| l.as_str())
                .unwrap_or("?");
            format!("[thinking: {level}]")
        }
        other => format!("[{other}]"),
    }
}

pub(super) fn truncate_str(s: &str, max_len: usize) -> String {
    let first_line = s.lines().next().unwrap_or(s);
    let chars: Vec<char> = first_line.chars().collect();
    if chars.len() <= max_len {
        first_line.to_string()
    } else {
        let truncated: String = chars[..max_len].iter().collect();
        format!("{truncated}...")
    }
}
