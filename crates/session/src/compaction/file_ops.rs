use std::collections::HashSet;

use bb_core::types::{AgentMessage, AssistantContent};

// =============================================================================
// File operation tracking
// =============================================================================

/// Extract read/modified files from messages by looking at tool calls.
pub fn extract_file_operations(messages: &[AgentMessage]) -> (Vec<String>, Vec<String>) {
    let mut read_files = HashSet::new();
    let mut modified_files = HashSet::new();

    for msg in messages {
        match msg {
            AgentMessage::Assistant(a) => {
                for block in &a.content {
                    if let AssistantContent::ToolCall { name, arguments, .. } = block {
                        match name.as_str() {
                            "read" => {
                                if let Some(path) = arguments.get("path").and_then(|v| v.as_str()) {
                                    read_files.insert(path.to_string());
                                }
                            }
                            "edit" | "write" => {
                                if let Some(path) = arguments.get("path").and_then(|v| v.as_str()) {
                                    modified_files.insert(path.to_string());
                                }
                            }
                            "bash" => {
                                if let Some(cmd) = arguments.get("command").and_then(|v| v.as_str()) {
                                    extract_bash_file_ops(cmd, &mut modified_files);
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            _ => {}
        }
    }

    let mut read_vec: Vec<String> = read_files.into_iter().collect();
    let mut mod_vec: Vec<String> = modified_files.into_iter().collect();
    read_vec.sort();
    mod_vec.sort();
    (read_vec, mod_vec)
}

/// Best-effort extraction of modified files from bash commands.
fn extract_bash_file_ops(cmd: &str, modified: &mut HashSet<String>) {
    // Detect redirect operators: > file, >> file
    for part in cmd.split_whitespace() {
        if part.starts_with('>') {
            let file = part.trim_start_matches('>');
            if !file.is_empty() {
                modified.insert(file.to_string());
            }
        }
    }
    // Detect "> file" pattern (space after >)
    let chars: Vec<char> = cmd.chars().collect();
    for i in 0..chars.len() {
        if chars[i] == '>' && (i == 0 || chars[i - 1] != '>') {
            // Skip >> (already handled above for combined token)
            let rest = &cmd[i + 1..];
            let rest = rest.trim_start_matches('>');
            let rest = rest.trim_start();
            if let Some(file) = rest.split_whitespace().next() {
                if !file.is_empty() && !file.starts_with('&') {
                    modified.insert(file.to_string());
                }
            }
        }
    }
    // Detect tee command
    if cmd.contains("tee ") {
        if let Some(pos) = cmd.find("tee ") {
            let after = &cmd[pos + 4..];
            // Skip flags
            for token in after.split_whitespace() {
                if token.starts_with('-') {
                    continue;
                }
                modified.insert(token.to_string());
                break;
            }
        }
    }
}

