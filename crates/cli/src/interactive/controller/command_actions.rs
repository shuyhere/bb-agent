use super::*;

/// Simple base64 encoder for OSC 52 clipboard.
fn base64_encode_simple(data: &[u8]) -> String {
    use base64::{Engine, engine::general_purpose::STANDARD};
    STANDARD.encode(data)
}

/// Pre-computed preview for a session entry.
struct EntryPreview {
    entry_type: String,
    preview: String,
}

/// Load all entry previews for a session in a single DB query.
fn load_all_previews(
    conn: &rusqlite::Connection,
    session_id: &str,
) -> std::collections::HashMap<String, EntryPreview> {
    let mut map = std::collections::HashMap::new();
    let rows = match store::get_entries(conn, session_id) {
        Ok(r) => r,
        Err(_) => return map,
    };
    for row in rows {
        let entry = match store::parse_entry(&row) {
            Ok(e) => e,
            Err(_) => continue,
        };
        let (entry_type, preview) = match &entry {
            bb_core::types::SessionEntry::Message { message, .. } => match message {
                bb_core::types::AgentMessage::User(u) => {
                    let text: String = u
                        .content
                        .iter()
                        .filter_map(|b| match b {
                            bb_core::types::ContentBlock::Text { text } => Some(text.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join(" ");
                    let line = text.trim().replace('\n', " ");
                    (
                        "user",
                        if line.is_empty() {
                            "(empty)".into()
                        } else {
                            line
                        },
                    )
                }
                bb_core::types::AgentMessage::Assistant(a) => {
                    let text = bb_core::agent::extract_text(&a.content);
                    let line = text.trim().replace('\n', " ");
                    (
                        "assistant",
                        if line.is_empty() {
                            "(empty)".into()
                        } else {
                            line
                        },
                    )
                }
                bb_core::types::AgentMessage::ToolResult(t) => {
                    ("tool_result", format!("{}: ...", t.tool_name))
                }
                bb_core::types::AgentMessage::CompactionSummary(_) => {
                    ("compaction", "[compaction summary]".into())
                }
                _ => ("other", "...".into()),
            },
            _ => ("other", "...".into()),
        };
        map.insert(
            row.entry_id.clone(),
            EntryPreview {
                entry_type: entry_type.into(),
                preview,
            },
        );
    }
    map
}

/// Flatten a tree into display entries with connectors.
fn flatten_tree(
    nodes: &[bb_session::tree::TreeNode],
    previews: &std::collections::HashMap<String, EntryPreview>,
    depth: usize,
    prefix: &str,
    leaf_id: &Option<String>,
    out: &mut Vec<FlatTreeEntry>,
    leaf_idx: &mut Option<usize>,
) {
    for (i, node) in nodes.iter().enumerate() {
        let is_last = i == nodes.len() - 1;
        let connector = if depth == 0 {
            String::new()
        } else {
            let branch = if is_last {
                "\u{2514}\u{2500} "
            } else {
                "\u{251c}\u{2500} "
            };
            format!("{prefix}{branch}")
        };

        let default_preview = EntryPreview {
            entry_type: "unknown".into(),
            preview: "(missing)".into(),
        };
        let ep = previews.get(&node.entry_id).unwrap_or(&default_preview);

        let is_leaf = leaf_id.as_deref() == Some(&node.entry_id);
        let is_branch_point = node.children.len() > 1;

        if is_leaf {
            *leaf_idx = Some(out.len());
        }

        out.push(FlatTreeEntry {
            entry_id: node.entry_id.clone(),
            entry_type: ep.entry_type.clone(),
            preview: ep.preview.clone(),
            timestamp: node.timestamp.clone(),
            indent: depth,
            is_leaf,
            is_branch_point,
            connector,
        });

        let child_prefix = if depth == 0 {
            String::new()
        } else {
            let cont = if is_last { "   " } else { "\u{2502}  " };
            format!("{prefix}{cont}")
        };

        flatten_tree(
            &node.children,
            previews,
            depth + 1,
            &child_prefix,
            leaf_id,
            out,
            leaf_idx,
        );
    }
}

/// Load the first user message and concatenated message text for a session.
fn load_session_message_preview(conn: &rusqlite::Connection, session_id: &str) -> (String, String) {
    let rows = match store::get_entries(conn, session_id) {
        Ok(r) => r,
        Err(_) => return (String::new(), String::new()),
    };

    let mut first_user_message = String::new();
    let mut all_text = Vec::new();

    for row in rows {
        let entry = match store::parse_entry(&row) {
            Ok(e) => e,
            Err(_) => continue,
        };
        if let bb_core::types::SessionEntry::Message { message, .. } = entry {
            match &message {
                bb_core::types::AgentMessage::User(u) => {
                    let text: String = u
                        .content
                        .iter()
                        .filter_map(|b| match b {
                            bb_core::types::ContentBlock::Text { text } => Some(text.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join(" ");
                    if !text.trim().is_empty() {
                        if first_user_message.is_empty() {
                            first_user_message = text.trim().replace('\n', " ");
                        }
                        all_text.push(text);
                    }
                }
                bb_core::types::AgentMessage::Assistant(a) => {
                    let text = bb_core::agent::extract_text(&a.content);
                    if !text.trim().is_empty() {
                        all_text.push(text);
                    }
                }
                _ => {}
            }
        }
    }

    (first_user_message, all_text.join(" "))
}

impl InteractiveMode {
    fn text_from_blocks(blocks: &[bb_core::types::ContentBlock], separator: &str) -> String {
        blocks
            .iter()
            .filter_map(|block| match block {
                bb_core::types::ContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(separator)
    }

    fn interactive_session_context_from_core(
        context: bb_core::types::SessionContext,
    ) -> super::super::events::SessionContext {
        use super::super::components::assistant_message::{
            AssistantMessage as UiAssistantMessage, AssistantMessageContent, AssistantStopReason,
        };
        use super::super::components::tool_execution::{ToolExecutionResult, ToolResultBlock};
        use super::super::events::{InteractiveMessage, SessionContext, ToolCallContent};

        let messages = context
            .messages
            .into_iter()
            .filter_map(|message| match message {
                bb_core::types::AgentMessage::User(user) => Some(InteractiveMessage::User {
                    text: Self::text_from_blocks(&user.content, "\n"),
                }),
                bb_core::types::AgentMessage::Assistant(message) => {
                    let mut content = Vec::new();
                    let mut tool_calls = Vec::new();
                    for block in message.content {
                        match block {
                            bb_core::types::AssistantContent::Text { text } => {
                                content.push(AssistantMessageContent::Text(text));
                            }
                            bb_core::types::AssistantContent::Thinking { thinking } => {
                                content.push(AssistantMessageContent::Thinking(thinking));
                            }
                            bb_core::types::AssistantContent::ToolCall {
                                id,
                                name,
                                arguments,
                            } => {
                                tool_calls.push(ToolCallContent {
                                    id,
                                    name,
                                    arguments,
                                });
                                content.push(AssistantMessageContent::ToolCall);
                            }
                        }
                    }
                    let stop_reason = Some(match message.stop_reason {
                        bb_core::types::StopReason::Aborted => AssistantStopReason::Aborted,
                        bb_core::types::StopReason::Error => AssistantStopReason::Error,
                        _ => AssistantStopReason::Other,
                    });
                    Some(InteractiveMessage::Assistant {
                        message: UiAssistantMessage {
                            content,
                            stop_reason,
                            error_message: message.error_message,
                        },
                        tool_calls,
                    })
                }
                bb_core::types::AgentMessage::ToolResult(result) => {
                    Some(InteractiveMessage::ToolResult {
                        tool_call_id: result.tool_call_id,
                        result: ToolExecutionResult {
                            content: result
                                .content
                                .into_iter()
                                .map(|block| match block {
                                    bb_core::types::ContentBlock::Text { text } => {
                                        ToolResultBlock {
                                            r#type: "text".to_string(),
                                            text: Some(text),
                                            data: None,
                                            mime_type: None,
                                        }
                                    }
                                    bb_core::types::ContentBlock::Image { data, mime_type } => {
                                        ToolResultBlock {
                                            r#type: "image".to_string(),
                                            text: None,
                                            data: Some(data),
                                            mime_type: Some(mime_type),
                                        }
                                    }
                                })
                                .collect(),
                            is_error: result.is_error,
                            details: result.details,
                        },
                    })
                }
                bb_core::types::AgentMessage::BashExecution(message) => {
                    Some(InteractiveMessage::BashExecution {
                        command: message.command,
                        output: Some(message.output),
                        exit_code: message.exit_code,
                        cancelled: message.cancelled,
                        truncated: message.truncated,
                        full_output_path: message.full_output_path,
                        exclude_from_context: false,
                    })
                }
                bb_core::types::AgentMessage::Custom(message) => Some(InteractiveMessage::Custom {
                    custom_type: message.custom_type,
                    text: Self::text_from_blocks(&message.content, "\n"),
                    display: message.display,
                }),
                bb_core::types::AgentMessage::BranchSummary(message) => {
                    Some(InteractiveMessage::BranchSummary {
                        summary: message.summary,
                    })
                }
                bb_core::types::AgentMessage::CompactionSummary(message) => {
                    Some(InteractiveMessage::CompactionSummary {
                        summary: message.summary,
                    })
                }
            })
            .collect();

        SessionContext { messages }
    }

    fn reset_rendered_session_state(&mut self) {
        self.clear_chat_items();
        self.invalidate_chat_cache();
        self.render_state_mut().pending_items.clear();
        self.render_state_mut().streaming_component = None;
        self.render_state_mut().streaming_message = None;
        self.render_state_mut().pending_tools.clear();
        self.streaming.streaming_text.clear();
        self.streaming.streaming_thinking.clear();
        self.streaming.streaming_tool_calls.clear();
        self.streaming.is_streaming = false;
        self.queues.steering_queue.clear();
        self.queues.follow_up_queue.clear();
        self.queues.compaction_queued_messages.clear();
        self.queues.pending_bash_components.clear();
    }

    pub(super) fn render_current_session_state(&mut self) {
        self.render_state_mut().tool_output_expanded = self.interaction.tool_output_expanded;
        self.render_state_mut().hide_thinking_block = self.streaming.hide_thinking_block;
        self.render_state_mut().hidden_thinking_label =
            self.streaming.hidden_thinking_label.clone();
        match bb_session::context::build_context(
            &self.session_setup.conn,
            &self.session_setup.session_id,
        ) {
            Ok(context) => {
                let context = Self::interactive_session_context_from_core(context);
                self.render_chat_from_session_context(&context);
            }
            Err(_) => self.clear_chat_items(),
        }
    }

    pub(super) fn rebuild_chat_from_session_with_live_components(&mut self) {
        let streaming_component = self.render_state().streaming_component.clone();
        let pending_tools = self.render_state().pending_tools.clone();

        self.render_current_session_state();

        self.render_state_mut().pending_tools = pending_tools.clone();

        if let Some(component) = streaming_component {
            self.append_chat_item(super::super::events::ChatItem::AssistantMessage(component));
        }
        for component in pending_tools.into_values() {
            self.append_chat_item(super::super::events::ChatItem::ToolExecution(component));
        }
    }

    pub(super) fn handle_export_command(&mut self, text: &str) {
        let path = text.strip_prefix("/export").unwrap_or("").trim();
        let file_path = if path.is_empty() {
            let ts = chrono::Utc::now().format("%Y-%m-%dT%H-%M-%S");
            format!("session-{ts}.jsonl")
        } else {
            path.to_string()
        };

        // Export current branch as JSONL (matches pi format).
        let entries = match bb_session::tree::active_path(
            &self.session_setup.conn,
            &self.session_setup.session_id,
        ) {
            Ok(e) => e,
            Err(e) => {
                self.show_warning(format!("Failed to read session: {e}"));
                return;
            }
        };

        let mut lines = Vec::new();

        // Session header
        let header = serde_json::json!({
            "type": "session",
            "version": 1,
            "id": self.session_setup.session_id,
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "cwd": self.session_setup.tool_ctx.cwd.display().to_string(),
        });
        lines.push(serde_json::to_string(&header).unwrap_or_default());

        // Linearize entries (re-chain parentIds)
        let mut prev_id: Option<String> = None;
        for row in &entries {
            if let Ok(entry) = store::parse_entry(row) {
                let mut val = serde_json::to_value(&entry).unwrap_or_default();
                if let Some(obj) = val.as_object_mut() {
                    obj.insert(
                        "parentId".into(),
                        match &prev_id {
                            Some(id) => serde_json::Value::String(id.clone()),
                            None => serde_json::Value::Null,
                        },
                    );
                }
                prev_id = Some(row.entry_id.clone());
                lines.push(serde_json::to_string(&val).unwrap_or_default());
            }
        }

        match std::fs::write(&file_path, format!("{}\n", lines.join("\n"))) {
            Ok(()) => {
                let abs = std::fs::canonicalize(&file_path)
                    .unwrap_or_else(|_| std::path::PathBuf::from(&file_path));
                self.show_status(format!("Session exported to: {}", abs.display()));
            }
            Err(e) => self.show_warning(format!("Failed to export: {e}")),
        }
    }

    pub(super) fn handle_import_command(&mut self, text: &str) {
        let path = text.strip_prefix("/import").unwrap_or("").trim();
        if path.is_empty() {
            self.show_warning("Usage: /import <path.jsonl>");
            return;
        }

        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                self.show_warning(format!("Failed to read {path}: {e}"));
                return;
            }
        };

        // Parse JSONL lines
        let mut entries: Vec<serde_json::Value> = Vec::new();
        let mut session_header: Option<serde_json::Value> = None;
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            match serde_json::from_str::<serde_json::Value>(line) {
                Ok(val) => {
                    if val.get("type").and_then(|t| t.as_str()) == Some("session") {
                        session_header = Some(val);
                    } else {
                        entries.push(val);
                    }
                }
                Err(e) => {
                    self.show_warning(format!("Invalid JSON line: {e}"));
                    return;
                }
            }
        }

        if entries.is_empty() {
            self.show_warning("No entries found in JSONL file.");
            return;
        }

        // Create a new session and import entries
        let cwd = self.session_setup.tool_ctx.cwd.display().to_string();
        let new_id = match store::create_session(&self.session_setup.conn, &cwd) {
            Ok(id) => id,
            Err(e) => {
                self.show_warning(format!("Failed to create session: {e}"));
                return;
            }
        };

        let mut imported = 0;
        for val in &entries {
            // Try to parse as SessionEntry
            if let Ok(entry) = serde_json::from_value::<bb_core::types::SessionEntry>(val.clone()) {
                if store::append_entry(&self.session_setup.conn, &new_id, &entry).is_ok() {
                    imported += 1;
                }
            }
        }

        if imported == 0 {
            self.show_warning("No valid entries imported.");
            return;
        }

        // Switch to the imported session
        self.session_setup.session_id = new_id.clone();
        self.session_setup.session_created = true;
        self.options.session_id = Some(new_id);

        self.reset_rendered_session_state();
        self.render_current_session_state();
        self.rebuild_pending_container();
        self.rebuild_footer();
        self.show_status(format!("Imported {imported} entries from {path}"));
        self.snapshot_chat_cache();
        self.refresh_ui();
    }

    pub(super) fn handle_copy_command(&mut self) {
        // Find the last assistant message text and copy to clipboard via OSC 52.
        let mut last_text = String::new();
        for item in self.render_state().chat_items.iter().rev() {
            if let ChatItem::AssistantMessage(component) = item {
                if let Some(msg) = component.last_message() {
                    for c in &msg.content {
                        if let super::super::components::assistant_message::AssistantMessageContent::Text(t) = c {
                            last_text = t.clone();
                            break;
                        }
                    }
                }
                if !last_text.is_empty() {
                    break;
                }
            }
        }
        if last_text.is_empty() {
            self.show_warning("No assistant messages to copy.");
            return;
        }
        // OSC 52 clipboard copy
        let encoded = base64_encode_simple(last_text.as_bytes());
        print!("\x1b]52;c;{encoded}\x07");
        self.show_status("Copied last assistant message to clipboard");
    }

    pub(super) fn handle_name_command(&mut self, text: &str) {
        let name = text.strip_prefix("/name").unwrap_or(text).trim();
        if name.is_empty() {
            // Show current name if called without args.
            let current =
                store::get_session(&self.session_setup.conn, &self.session_setup.session_id)
                    .ok()
                    .flatten()
                    .and_then(|r| r.name);
            match current {
                Some(n) => {
                    self.add_chat_message(super::super::events::InteractiveMessage::System {
                        text: format!("Session name: {n}"),
                    });
                    self.snapshot_chat_cache();
                    self.refresh_ui();
                }
                None => self.show_status("Usage: /name <name>"),
            }
            return;
        }
        match store::set_session_name(
            &self.session_setup.conn,
            &self.session_setup.session_id,
            Some(name),
        ) {
            Ok(_) => {
                self.add_chat_message(super::super::events::InteractiveMessage::System {
                    text: format!("Session name set: {name}"),
                });
                self.rebuild_footer();
                self.snapshot_chat_cache();
                self.refresh_ui();
            }
            Err(e) => self.show_warning(format!("Failed to rename session: {e}")),
        }
    }

    pub(super) fn handle_session_command(&mut self) {
        let bold = "\x1b[1m";
        let dim = "\x1b[2m";
        let reset = "\x1b[0m";

        let session_id = &self.session_setup.session_id;
        let session_row = store::get_session(&self.session_setup.conn, session_id)
            .ok()
            .flatten();
        let session_name = session_row.as_ref().and_then(|r| r.name.as_deref());
        let session_file = self
            .session_setup
            .conn
            .path()
            .map(|p| p.to_string())
            .unwrap_or_else(|| "in-memory".into());

        // Count messages by role from session entries
        let mut user_msgs = 0_u64;
        let mut asst_msgs = 0_u64;
        let mut tool_calls = 0_u64;
        let mut tool_results = 0_u64;
        let mut total_input = 0_u64;
        let mut total_output = 0_u64;
        let mut total_cache_read = 0_u64;
        let mut total_cache_write = 0_u64;
        let mut total_cost = 0.0_f64;

        if let Ok(rows) = store::get_entries(&self.session_setup.conn, session_id) {
            for row in rows {
                if let Ok(entry) = store::parse_entry(&row) {
                    match &entry {
                        bb_core::types::SessionEntry::Message { message, .. } => match message {
                            bb_core::types::AgentMessage::User(_) => user_msgs += 1,
                            bb_core::types::AgentMessage::Assistant(a) => {
                                asst_msgs += 1;
                                tool_calls += a
                                    .content
                                    .iter()
                                    .filter(|c| {
                                        matches!(
                                            c,
                                            bb_core::types::AssistantContent::ToolCall { .. }
                                        )
                                    })
                                    .count() as u64;
                                total_input += a.usage.input;
                                total_output += a.usage.output;
                                total_cache_read += a.usage.cache_read;
                                total_cache_write += a.usage.cache_write;
                                total_cost += a.usage.cost.total;
                            }
                            bb_core::types::AgentMessage::ToolResult(_) => tool_results += 1,
                            _ => {}
                        },
                        _ => {}
                    }
                }
            }
        }

        let total_msgs = user_msgs + asst_msgs + tool_results;
        let total_tokens = total_input + total_output + total_cache_read + total_cache_write;

        let mut info = format!("{bold}Session Info{reset}\n\n");
        if let Some(name) = session_name {
            info.push_str(&format!("{dim}Name:{reset} {name}\n"));
        }
        info.push_str(&format!("{dim}File:{reset} {session_file}\n"));
        info.push_str(&format!("{dim}ID:{reset} {session_id}\n\n"));

        info.push_str(&format!("{bold}Messages{reset}\n"));
        info.push_str(&format!("{dim}User:{reset} {user_msgs}\n"));
        info.push_str(&format!("{dim}Assistant:{reset} {asst_msgs}\n"));
        info.push_str(&format!("{dim}Tool Calls:{reset} {tool_calls}\n"));
        info.push_str(&format!("{dim}Tool Results:{reset} {tool_results}\n"));
        info.push_str(&format!("{dim}Total:{reset} {total_msgs}\n\n"));

        info.push_str(&format!("{bold}Tokens{reset}\n"));
        info.push_str(&format!("{dim}Input:{reset} {total_input}\n"));
        info.push_str(&format!("{dim}Output:{reset} {total_output}\n"));
        if total_cache_read > 0 {
            info.push_str(&format!("{dim}Cache Read:{reset} {total_cache_read}\n"));
        }
        if total_cache_write > 0 {
            info.push_str(&format!("{dim}Cache Write:{reset} {total_cache_write}\n"));
        }
        info.push_str(&format!("{dim}Total:{reset} {total_tokens}\n"));

        if total_cost > 0.0 {
            info.push_str(&format!("\n{bold}Cost{reset}\n"));
            info.push_str(&format!("{dim}Total:{reset} ${total_cost:.4}"));
        }

        self.add_chat_message(super::super::events::InteractiveMessage::System { text: info });
        self.snapshot_chat_cache();
        self.refresh_ui();
    }

    pub(super) fn handle_changelog_command(&mut self) {
        self.add_chat_message(super::super::events::InteractiveMessage::System {
            text: format!(
                "BB-Agent v{}\n\nNo changelog entries yet.",
                env!("CARGO_PKG_VERSION")
            ),
        });
        self.snapshot_chat_cache();
        self.refresh_ui();
    }

    pub(super) fn handle_hotkeys_command(&mut self) {
        let hotkeys = vec![
            "Key Bindings:",
            "  Ctrl+C      - Interrupt / clear input",
            "  Ctrl+D      - Exit (on empty input)",
            "  Ctrl+Z      - Suspend",
            "  Ctrl+J      - Cycle thinking level",
            "  Ctrl+K      - Cycle model forward",
            "  Ctrl+L      - Toggle tool output expansion",
            "  Ctrl+T      - Toggle thinking visibility",
            "  Ctrl+E      - Open external editor",
            "  Ctrl+R      - Resume session selector",
            "  Ctrl+N      - New session",
            "  Ctrl+F      - Follow-up message",
            "  Ctrl+V      - Paste image from clipboard",
            "  Esc         - Cancel / back",
        ];
        for line in hotkeys {
            self.render_cache.chat_lines.push(line.to_string());
        }
    }

    pub(super) fn show_user_message_selector(&mut self) {
        // Collect user messages for forking.
        let mut user_msgs: Vec<(String, String)> = Vec::new(); // (entry_id, text)
        if let Ok(rows) =
            store::get_entries(&self.session_setup.conn, &self.session_setup.session_id)
        {
            for row in rows {
                if let Ok(entry) = store::parse_entry(&row) {
                    if let bb_core::types::SessionEntry::Message {
                        base,
                        message: bb_core::types::AgentMessage::User(u),
                        ..
                    } = entry
                    {
                        let text: String = u
                            .content
                            .iter()
                            .filter_map(|b| match b {
                                bb_core::types::ContentBlock::Text { text } => Some(text.as_str()),
                                _ => None,
                            })
                            .collect::<Vec<_>>()
                            .join(" ");
                        let text = text.trim().replace('\n', " ");
                        if !text.is_empty() {
                            user_msgs.push((base.id.0, text));
                        }
                    }
                }
            }
        }

        if user_msgs.is_empty() {
            self.show_status("No messages to fork from");
            return;
        }

        // Build session list items to reuse the session selector overlay.
        let items: Vec<SessionListItem> = user_msgs
            .iter()
            .enumerate()
            .map(|(i, (id, text))| {
                SessionListItem {
                    session_id: id.clone(), // reuse session_id field for entry_id
                    name: None,
                    cwd: String::new(),
                    updated_at: String::new(),
                    entry_count: (i + 1) as i64,
                    is_current: false,
                    first_message: text.clone(),
                    all_messages_text: text.clone(),
                }
            })
            .collect();

        // We'll use a simple overlay — store the fork intent so process_overlay_actions
        // knows to handle it as a fork rather than a resume.
        self.interaction.pending_fork = true;
        let overlay = Box::new(SessionSelectorOverlay::new(items));
        self.ui.tui.show_overlay(overlay);
        self.ui.tui.force_render();
        self.show_status("Select a user message to fork from");
    }

    pub(super) fn handle_fork_from_entry(&mut self, entry_id: &str) {
        // Fork = move the leaf to the parent of the selected user message,
        // so the next prompt creates a new branch.
        // Also put the selected message text back in the editor.

        // Get the entry to find parent and text.
        let (parent_id, editor_text) = match store::get_entry(
            &self.session_setup.conn,
            &self.session_setup.session_id,
            entry_id,
        ) {
            Ok(Some(row)) => {
                let text = match store::parse_entry(&row) {
                    Ok(bb_core::types::SessionEntry::Message {
                        message: bb_core::types::AgentMessage::User(u),
                        ..
                    }) => u
                        .content
                        .iter()
                        .filter_map(|b| match b {
                            bb_core::types::ContentBlock::Text { text } => Some(text.clone()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("\n"),
                    _ => String::new(),
                };
                (row.parent_id, text)
            }
            _ => {
                self.show_warning("Entry not found");
                return;
            }
        };

        // Set leaf to the parent of the user message (the branch point).
        if let Err(e) = store::set_leaf(
            &self.session_setup.conn,
            &self.session_setup.session_id,
            parent_id.as_deref(),
        ) {
            self.show_warning(format!("Failed to fork: {e}"));
            return;
        }

        // Clear and re-render from the new position.
        self.reset_rendered_session_state();
        self.render_current_session_state();
        self.rebuild_pending_container();
        self.rebuild_footer();

        // Put the user message text back in editor for editing.
        if !editor_text.trim().is_empty() {
            self.set_editor_text(&editor_text);
        }

        self.show_status("Forked — edit and send to create a new branch");
    }

    pub(super) fn show_tree_selector(&mut self) {
        let tree =
            bb_session::tree::get_tree(&self.session_setup.conn, &self.session_setup.session_id);
        let tree = match tree {
            Ok(t) => t,
            Err(e) => {
                self.show_warning(format!("Failed to load tree: {e}"));
                return;
            }
        };

        if tree.is_empty() {
            self.show_status("No entries in session");
            return;
        }

        let leaf_id = self.get_session_leaf().map(|id| id.0);

        // Load all entry previews in one query (not per-node)
        let previews = load_all_previews(&self.session_setup.conn, &self.session_setup.session_id);

        // Flatten tree into display entries
        let mut flat: Vec<FlatTreeEntry> = Vec::new();
        let mut leaf_idx = None;
        flatten_tree(&tree, &previews, 0, "", &leaf_id, &mut flat, &mut leaf_idx);

        let overlay = Box::new(TreeSelectorOverlay::new(flat, leaf_idx));
        self.clear_status();
        self.ui.tui.show_overlay(overlay);
        self.ui.tui.force_render();
    }

    pub(super) fn handle_clear_command(&mut self) {
        let _ = self.controller.runtime_host.session_mut().clear_queue();
        self.render_cache.chat_lines.clear();
        self.render_cache.pending_lines.clear();
        self.queues.steering_queue.clear();
        self.queues.follow_up_queue.clear();
        self.queues.compaction_queued_messages.clear();
        self.clear_chat_items();
        self.invalidate_chat_cache();
        self.render_state_mut().pending_items.clear();
        self.show_status("Started a fresh interactive session shell around the core session");
    }

    pub(super) fn handle_new_session(&mut self) {
        // Don't create the DB row yet — lazy create on first message,
        // just like startup. This avoids empty /new sessions.
        let new_id = uuid::Uuid::new_v4().to_string();
        {
            self.session_setup.session_id = new_id.clone();
            self.session_setup.session_created = false; // lazy
            self.options.session_id = Some(new_id.clone());
            let _ = self.controller.runtime_host.session_mut().clear_queue();

            // Clear all chat/pending/streaming state (match pi's renderCurrentSessionState)
            self.reset_rendered_session_state();
            self.rebuild_pending_container();
            self.rebuild_footer();

            // Show confirmation (like pi's "New session started")
            self.add_chat_message(super::super::events::InteractiveMessage::System {
                text: "New session started".to_string(),
            });
            self.snapshot_chat_cache();
            self.refresh_ui();
        }
    }

    pub(super) fn handle_help_command(&mut self) {
        let help = [
            "\x1b[1mCommands\x1b[0m",
            "  /help           Show this help",
            "  /model          Switch model (Ctrl+L)",
            "  /settings       Open settings",
            "  /login          Login to provider",
            "  /logout         Logout from provider",
            "  /new            New session (Ctrl+N)",
            "  /resume         Resume a session (Ctrl+R)",
            "  /name <name>    Set session name",
            "  /session        Show session stats",
            "  /tree           Session tree navigator",
            "  /fork           Fork from a user message",
            "  /compact        Compact context",
            "  /copy           Copy last response to clipboard",
            "  /hotkeys        Show keyboard shortcuts",
            "  /debug          Write debug log",
            "  /changelog      Show changelog",
            "  /quit           Exit",
            "",
            "\x1b[1mBash\x1b[0m",
            "  !<cmd>          Run bash command",
            "  !!<cmd>         Run bash (excluded from context)",
            "",
            "\x1b[1mEditor\x1b[0m",
            "  Enter           Send message",
            "  Alt+Enter       New line",
            "  Esc             Cancel / back",
            "  Ctrl+C          Clear / interrupt",
            "  @               File autocomplete",
        ]
        .join("\n");
        self.add_chat_message(super::super::events::InteractiveMessage::System { text: help });
        self.snapshot_chat_cache();
        self.refresh_ui();
    }

    pub(super) fn check_auto_compaction(&mut self) {
        let session_id = self.session_setup.session_id.clone();
        let settings = bb_core::types::CompactionSettings::default();
        if let Ok(entries) = store::get_entries(&self.session_setup.conn, &session_id) {
            let total_tokens: u64 = entries.iter().map(compaction::estimate_tokens_row).sum();
            let window = self.session_setup.model.context_window;
            if compaction::should_compact(total_tokens, window, &settings) {
                self.render_cache.chat_lines.push(format!(
                    "[c] Auto-compaction triggered ({total_tokens} tokens, window {window})"
                ));
                // Prepare and note - full async LLM summarization deferred to future wave
                if let Some(prep) = compaction::prepare_compaction(&entries, &settings) {
                    self.render_cache.chat_lines.push(format!(
                        "[c] {} messages to summarize, {} kept",
                        prep.messages_to_summarize.len(),
                        prep.kept_messages.len()
                    ));
                }
            }
        }
    }

    pub(super) fn handle_compact_command(&mut self, instructions: Option<&str>) {
        self.interaction.is_compacting = true;
        let session_id = self.session_setup.session_id.clone();
        match store::get_entries(&self.session_setup.conn, &session_id) {
            Ok(entries) => {
                let settings = bb_core::types::CompactionSettings::default();
                let total_tokens: u64 = entries.iter().map(compaction::estimate_tokens_row).sum();
                match compaction::prepare_compaction(&entries, &settings) {
                    Some(prep) => {
                        let to_summarize = prep.messages_to_summarize.len();
                        let kept = prep.kept_messages.len();
                        self.render_cache.chat_lines.push(format!(
                            "Compaction: {total_tokens} estimated tokens, {to_summarize} messages to summarize, {kept} kept"
                        ));
                        if let Some(inst) = instructions {
                            self.render_cache
                                .chat_lines
                                .push(format!("Instructions: {inst}"));
                        }
                        self.show_status("Compaction prepared (async LLM summarization not wired in interactive mode yet)");
                    }
                    None => {
                        self.show_status(format!(
                            "Nothing to compact ({total_tokens} estimated tokens, {} entries)",
                            entries.len()
                        ));
                    }
                }
            }
            Err(e) => {
                self.show_status(format!("Failed to get entries for compaction: {e}"));
            }
        }
        self.interaction.is_compacting = false;
    }

    pub(super) async fn handle_reload_command(&mut self) -> InteractiveResult<()> {
        if self.streaming.is_streaming {
            self.show_warning("/reload is only available while the agent is idle");
            return Ok(());
        }
        if self.interaction.is_compacting {
            self.show_warning("/reload is not available while compaction is active");
            return Ok(());
        }

        let cwd = self.session_setup.tool_ctx.cwd.clone();
        let settings = bb_core::settings::Settings::load_merged(&cwd);
        let crate::extensions::RuntimeExtensionSupport {
            session_resources,
            mut tools,
            commands,
        } = crate::extensions::load_runtime_extension_support_with_ui(
            &cwd,
            &settings,
            &self.session_setup.extension_bootstrap,
            true,
        )
        .await
        .map_err(|err| -> Box<dyn Error + Send + Sync> {
            Box::new(std::io::Error::other(err.to_string()))
        })?;

        let mut builtin_tools = bb_tools::builtin_tools();
        builtin_tools.append(&mut tools);
        self.session_setup.tool_defs = builtin_tools
            .iter()
            .map(|t| {
                serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": t.name(),
                        "description": t.description(),
                        "parameters": t.parameters_schema(),
                    }
                })
            })
            .collect();
        self.session_setup.tools = builtin_tools;
        self.session_setup.extension_commands = commands;
        self.controller
            .runtime_host
            .reload_resources(session_resources);
        let _ = self
            .session_setup
            .extension_commands
            .send_event(&bb_hooks::Event::SessionStart)
            .await;
        self.show_status("Reloaded resources and extensions");
        Ok(())
    }

    pub(super) fn handle_debug_command(&mut self) {
        let width = self.ui.tui.columns();
        let lines = self.ui.tui.root.render(width);
        let debug_path = bb_core::config::global_dir().join("debug.log");
        let mut data = Vec::new();
        data.push(format!("Debug output at {}", chrono::Utc::now()));
        data.push(format!("Terminal width: {width}"));
        data.push(format!("Total lines: {}", lines.len()));
        data.push(String::new());
        data.push("=== Rendered lines ===".into());
        for (i, line) in lines.iter().enumerate() {
            let vw = bb_tui::utils::visible_width(line);
            let escaped = format!("{:?}", line);
            data.push(format!("[{i}] (w={vw}) {escaped}"));
        }
        data.push(String::new());
        data.push(format!("Session: {}", self.session_setup.session_id));
        data.push(format!(
            "Model: {}/{}",
            self.session_setup.model.provider, self.session_setup.model.id
        ));
        data.push(format!("Thinking: {}", self.session_setup.thinking_level));
        data.push(format!(
            "API key: {}...",
            &self
                .session_setup
                .api_key
                .chars()
                .take(10)
                .collect::<String>()
        ));
        let _ = std::fs::write(&debug_path, data.join("\n"));
        self.show_status(format!("Debug log written to {}", debug_path.display()));
    }

    pub(super) fn handle_armin_says_hi(&mut self) {
        self.add_chat_message(InteractiveMessage::Assistant {
            message: assistant_message_from_parts("hi armin 👋", None, false),
            tool_calls: Vec::new(),
        });
    }

    pub(super) fn show_session_selector(&mut self) {
        let cwd = self.session_setup.tool_ctx.cwd.display().to_string();
        let current_id = self.session_setup.session_id.clone();

        let sessions: Vec<SessionListItem> = store::list_sessions(&self.session_setup.conn, &cwd)
            .unwrap_or_default()
            .into_iter()
            .map(|row| {
                // Load first user message and all messages text for search.
                let (first_message, all_messages_text) =
                    load_session_message_preview(&self.session_setup.conn, &row.session_id);
                SessionListItem {
                    is_current: row.session_id == current_id,
                    session_id: row.session_id,
                    name: row.name,
                    cwd: row.cwd,
                    updated_at: row.updated_at,
                    entry_count: row.entry_count,
                    first_message,
                    all_messages_text,
                }
            })
            .collect();

        if sessions.is_empty() {
            self.show_status("No sessions found in this directory.");
            return;
        }

        let overlay = Box::new(SessionSelectorOverlay::new(sessions));
        self.clear_status();
        self.ui.tui.show_overlay(overlay);
        self.ui.tui.force_render();
    }

    pub(super) fn handle_resume_session(&mut self, session_id: &str) {
        // Match pi: clear transient status UI before switching sessions.
        self.streaming.status_loader = None;
        self.clear_status();

        // Switch the active session.
        self.session_setup.session_id = session_id.to_string();
        self.session_setup.session_created = true; // already exists in DB
        self.options.session_id = Some(session_id.to_string());
        let _ = self.controller.runtime_host.session_mut().clear_queue();

        // Clear all chat/streaming state (like /new), then rebuild from session context.
        self.reset_rendered_session_state();
        self.render_current_session_state();

        // Match pi more closely: render history, then append a status line.
        self.show_status("Resumed session");
        self.snapshot_chat_cache();
        self.refresh_ui();
    }

    pub(super) fn handle_tree_navigate(&mut self, entry_id: &str) {
        let leaf_id = self.get_session_leaf().map(|id| id.0);
        if leaf_id.as_deref() == Some(entry_id) {
            self.show_status("Already at this point");
            return;
        }

        // Move the leaf pointer to the selected entry
        if let Err(e) = store::set_leaf(
            &self.session_setup.conn,
            &self.session_setup.session_id,
            Some(entry_id),
        ) {
            self.show_warning(format!("Failed to navigate: {e}"));
            return;
        }

        // Clear and re-render from the new position
        self.reset_rendered_session_state();
        self.render_current_session_state();
        self.rebuild_pending_container();
        self.rebuild_footer();

        self.show_status("Navigated to selected point");
        self.snapshot_chat_cache();
        self.refresh_ui();
    }

    pub(super) fn shutdown(&mut self) {
        self.interaction.shutdown_requested = true;
        self.show_status("Shutdown requested");
    }

    pub(super) fn handle_bash_command(&mut self, command: &str, excluded_from_context: bool) {
        let label = if excluded_from_context {
            "bash(excluded)"
        } else {
            "bash"
        };
        self.render_cache
            .chat_lines
            .push(format!("{label}> {command}"));
        let output = std::process::Command::new("sh")
            .arg("-c")
            .arg(command)
            .current_dir(&self.session_setup.tool_ctx.cwd)
            .output();
        match output {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                let stderr = String::from_utf8_lossy(&out.stderr);
                if !stdout.is_empty() {
                    for line in stdout.lines() {
                        self.render_cache.chat_lines.push(line.to_string());
                    }
                }
                if !stderr.is_empty() {
                    for line in stderr.lines() {
                        self.render_cache.chat_lines.push(format!("stderr: {line}"));
                    }
                }
                if !out.status.success() {
                    self.render_cache
                        .chat_lines
                        .push(format!("exit code: {}", out.status.code().unwrap_or(-1)));
                }
            }
            Err(e) => {
                self.render_cache
                    .chat_lines
                    .push(format!("Failed to execute command: {e}"));
            }
        }
    }

    pub(super) fn flush_pending_bash_components(&mut self) {
        while let Some(line) = self.queues.pending_bash_components.pop_front() {
            self.render_cache.chat_lines.push(line);
        }
    }

    pub(super) fn is_extension_command(&self, text: &str) -> bool {
        self.session_setup.extension_commands.is_registered(text)
            || self
                .controller
                .runtime_host
                .session()
                .is_extension_command_text(text)
    }

    pub(super) fn queue_compaction_message(&mut self, text: String, kind: QueuedMessageKind) {
        self.queues
            .compaction_queued_messages
            .push_back(QueuedMessage { text, kind });
        self.show_status("Queued message while compaction is active");
    }

    pub(super) fn show_model_selector(&mut self, initial_search: Option<&str>) {
        let current_model = self
            .controller
            .runtime_host
            .session()
            .model()
            .map(|m| format!("{}/{}", m.provider, m.id))
            .unwrap_or_else(|| {
                format!(
                    "{}/{}",
                    self.session_setup.model.provider, self.session_setup.model.id
                )
            });

        let mut models = self.get_model_candidates();
        let current_provider = self.session_setup.model.provider.clone();
        let current_id = self.session_setup.model.id.clone();
        models.sort_by(|a, b| {
            let a_current = a.provider == current_provider && a.id == current_id;
            let b_current = b.provider == current_provider && b.id == current_id;
            b_current
                .cmp(&a_current)
                .then_with(|| a.provider.cmp(&b.provider))
                .then_with(|| a.id.cmp(&b.id))
        });

        let mut selector = ModelSelector::from_models(models, 10);
        if let Some(query) = initial_search.filter(|s| !s.is_empty()) {
            selector.set_search(query);
        }
        let component = Box::new(ModelSelectorOverlay::new(
            selector,
            current_model,
            initial_search.map(|s| s.to_string()),
        ));
        self.clear_status();
        self.ui.tui.show_overlay(component);
        self.ui.tui.force_render();
    }

    pub(super) fn show_placeholder(&mut self, label: &str) {
        self.show_status(format!("TODO: {label}"));
    }
}
