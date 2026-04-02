use super::*;

/// Flatten a tree into display entries with connectors.
fn flatten_tree(
    nodes: &[bb_session::tree::TreeNode],
    conn: &rusqlite::Connection,
    session_id: &str,
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
            let branch = if is_last { "\u{2514}\u{2500} " } else { "\u{251c}\u{2500} " };
            format!("{prefix}{branch}")
        };

        // Get message preview
        let (entry_type, preview) = get_entry_preview(conn, session_id, &node.entry_id);

        let is_leaf = leaf_id.as_deref() == Some(&node.entry_id);
        let is_branch_point = node.children.len() > 1;

        if is_leaf {
            *leaf_idx = Some(out.len());
        }

        out.push(FlatTreeEntry {
            entry_id: node.entry_id.clone(),
            entry_type,
            preview,
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

        flatten_tree(&node.children, conn, session_id, depth + 1, &child_prefix, leaf_id, out, leaf_idx);
    }
}

/// Get entry type and a preview string for a tree node.
fn get_entry_preview(conn: &rusqlite::Connection, session_id: &str, entry_id: &str) -> (String, String) {
    let row = match store::get_entry(conn, session_id, entry_id) {
        Ok(Some(r)) => r,
        _ => return ("unknown".into(), "(missing)".into()),
    };
    let entry = match store::parse_entry(&row) {
        Ok(e) => e,
        Err(_) => return ("unknown".into(), "(parse error)".into()),
    };
    match entry {
        bb_core::types::SessionEntry::Message { message, .. } => match &message {
            bb_core::types::AgentMessage::User(u) => {
                let text: String = u.content.iter().filter_map(|b| match b {
                    bb_core::types::ContentBlock::Text { text } => Some(text.as_str()),
                    _ => None,
                }).collect::<Vec<_>>().join(" ");
                let line = text.trim().replace('\n', " ");
                ("user".into(), if line.is_empty() { "(empty)".into() } else { line })
            }
            bb_core::types::AgentMessage::Assistant(a) => {
                let text = bb_core::agent::extract_text(&a.content);
                let line = text.trim().replace('\n', " ");
                ("assistant".into(), if line.is_empty() { "(empty)".into() } else { line })
            }
            bb_core::types::AgentMessage::ToolResult(t) => {
                ("tool_result".into(), format!("{}: ...", t.tool_name))
            }
            bb_core::types::AgentMessage::CompactionSummary(_) => {
                ("compaction".into(), "[compaction summary]".into())
            }
            _ => ("other".into(), "...".into()),
        },
        _ => ("other".into(), "...".into()),
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
                    let text: String = u.content.iter().filter_map(|b| match b {
                        bb_core::types::ContentBlock::Text { text } => Some(text.as_str()),
                        _ => None,
                    }).collect::<Vec<_>>().join(" ");
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
    pub(super) fn handle_export_command(&mut self, text: &str) {
        self.show_status(format!("TODO: export command {text}"));
    }

    pub(super) fn handle_import_command(&mut self, text: &str) {
        self.show_status(format!("TODO: import command {text}"));
    }

    pub(super) fn handle_share_command(&mut self) {
        self.show_status("TODO: share command");
    }

    pub(super) fn handle_copy_command(&mut self) {
        self.show_status("TODO: copy command");
    }

    pub(super) fn handle_name_command(&mut self, text: &str) {
        let name = text.strip_prefix("/name").unwrap_or(text).trim();
        if name.is_empty() {
            // Show current name if called without args.
            let current = store::get_session(
                &self.session_setup.conn,
                &self.session_setup.session_id,
            ).ok().flatten().and_then(|r| r.name);
            match current {
                Some(n) => {
                    self.render_state_mut().add_message_to_chat(
                        super::super::events::InteractiveMessage::System {
                            text: format!("Session name: {n}"),
                        },
                    );
                    self.rebuild_chat_container();
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
                self.render_state_mut().add_message_to_chat(
                    super::super::events::InteractiveMessage::System {
                        text: format!("Session name set: {name}"),
                    },
                );
                self.rebuild_chat_container();
                self.rebuild_footer();
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
        let session_file = self.session_setup.conn
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
                                tool_calls += a.content.iter().filter(|c| {
                                    matches!(c, bb_core::types::AssistantContent::ToolCall { .. })
                                }).count() as u64;
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

        self.render_state_mut()
            .add_message_to_chat(super::super::events::InteractiveMessage::System {
                text: info,
            });
        self.rebuild_chat_container();
        self.refresh_ui();
    }

    pub(super) fn handle_changelog_command(&mut self) {
        self.show_status("TODO: changelog command");
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
        if let Ok(rows) = store::get_entries(&self.session_setup.conn, &self.session_setup.session_id) {
            for row in rows {
                if let Ok(entry) = store::parse_entry(&row) {
                    if let bb_core::types::SessionEntry::Message {
                        base, message: bb_core::types::AgentMessage::User(u), ..
                    } = entry {
                        let text: String = u.content.iter().filter_map(|b| match b {
                            bb_core::types::ContentBlock::Text { text } => Some(text.as_str()),
                            _ => None,
                        }).collect::<Vec<_>>().join(" ");
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
        let items: Vec<SessionListItem> = user_msgs.iter().enumerate().map(|(i, (id, text))| {
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
        }).collect();

        // We'll use a simple overlay — store the fork intent so process_overlay_actions
        // knows to handle it as a fork rather than a resume.
        self.interaction.pending_fork = true;
        let overlay = Box::new(SessionSelectorOverlay::new(items));
        self.ui.tui.show_overlay(overlay);
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
                        message: bb_core::types::AgentMessage::User(u), ..
                    }) => u.content.iter().filter_map(|b| match b {
                        bb_core::types::ContentBlock::Text { text } => Some(text.clone()),
                        _ => None,
                    }).collect::<Vec<_>>().join("\n"),
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
        self.render_state_mut().chat_items.clear();
        self.render_state_mut().pending_items.clear();
        self.render_state_mut().streaming_component = None;
        self.streaming.streaming_text.clear();
        self.streaming.streaming_thinking.clear();
        self.streaming.streaming_tool_calls.clear();
        self.streaming.is_streaming = false;

        // Re-render from root to new leaf.
        if let Ok(path) = bb_session::tree::active_path(
            &self.session_setup.conn,
            &self.session_setup.session_id,
        ) {
            for row in &path {
                if let Ok(entry) = store::parse_entry(row) {
                    match entry {
                        bb_core::types::SessionEntry::Message { message, .. } => match message {
                            bb_core::types::AgentMessage::User(u) => {
                                let text: String = u.content.iter().filter_map(|b| match b {
                                    bb_core::types::ContentBlock::Text { text } => Some(text.as_str()),
                                    _ => None,
                                }).collect::<Vec<_>>().join("\n");
                                self.render_state_mut().add_message_to_chat(
                                    super::super::events::InteractiveMessage::User { text },
                                );
                            }
                            bb_core::types::AgentMessage::Assistant(a) => {
                                use super::super::components::assistant_message::{
                                    AssistantMessage as AMsg, AssistantMessageContent,
                                };
                                let mut content = Vec::new();
                                for c in &a.content {
                                    match c {
                                        bb_core::types::AssistantContent::Text { text } => {
                                            content.push(AssistantMessageContent::Text(text.clone()));
                                        }
                                        bb_core::types::AssistantContent::Thinking { thinking } => {
                                            content.push(AssistantMessageContent::Thinking(thinking.clone()));
                                        }
                                        _ => {}
                                    }
                                }
                                let msg = AMsg { content, stop_reason: None, error_message: a.error_message.clone() };
                                self.render_state_mut().add_message_to_chat(
                                    super::super::events::InteractiveMessage::Assistant {
                                        message: msg, tool_calls: vec![],
                                    },
                                );
                            }
                            _ => {}
                        },
                        _ => {}
                    }
                }
            }
        }

        self.rebuild_chat_container();
        self.rebuild_pending_container();
        self.rebuild_footer();

        // Put the user message text back in editor for editing.
        if !editor_text.trim().is_empty() {
            self.set_editor_text(&editor_text);
        }

        self.show_status("Forked — edit and send to create a new branch");
    }

    pub(super) fn show_tree_selector(&mut self) {
        let tree = bb_session::tree::get_tree(
            &self.session_setup.conn,
            &self.session_setup.session_id,
        );
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

        // Flatten tree into display entries
        let mut flat: Vec<FlatTreeEntry> = Vec::new();
        let mut leaf_idx = None;
        flatten_tree(&tree, &self.session_setup.conn, &self.session_setup.session_id,
            0, "", &leaf_id, &mut flat, &mut leaf_idx);

        let overlay = Box::new(TreeSelectorOverlay::new(flat, leaf_idx));
        self.ui.tui.show_overlay(overlay);
    }

    pub(super) fn handle_clear_command(&mut self) {
        let _ = self.controller.runtime_host.session_mut().clear_queue();
        self.render_cache.chat_lines.clear();
        self.render_cache.pending_lines.clear();
        self.queues.steering_queue.clear();
        self.queues.follow_up_queue.clear();
        self.queues.compaction_queued_messages.clear();
        self.render_state_mut().chat_items.clear();
        self.render_state_mut().pending_items.clear();
        self.show_status("Started a fresh interactive session shell around the core session");
    }

    pub(super) fn handle_new_session(&mut self) {
        let cwd_str = self.session_setup.tool_ctx.cwd.display().to_string();
        match store::create_session(&self.session_setup.conn, &cwd_str) {
            Ok(new_id) => {
                self.session_setup.session_id = new_id.clone();
                self.options.session_id = Some(new_id.clone());
                let _ = self.controller.runtime_host.session_mut().clear_queue();

                // Clear all chat/pending/streaming state (match pi's renderCurrentSessionState)
                self.render_state_mut().chat_items.clear();
                self.render_state_mut().pending_items.clear();
                self.render_state_mut().streaming_component = None;
                self.streaming.streaming_text.clear();
                self.streaming.streaming_thinking.clear();
                self.streaming.streaming_tool_calls.clear();
                self.streaming.is_streaming = false;
                self.queues.steering_queue.clear();
                self.queues.follow_up_queue.clear();
                self.queues.compaction_queued_messages.clear();
                self.queues.pending_bash_components.clear();

                // Rebuild containers from scratch so TUI matches cleared state
                self.rebuild_chat_container();
                self.rebuild_pending_container();
                self.rebuild_footer();

                // Show confirmation (like pi's "New session started")
                self.render_state_mut()
                    .add_message_to_chat(super::super::events::InteractiveMessage::System {
                        text: "New session started".to_string(),
                    });
                self.rebuild_chat_container();
                self.refresh_ui();
            }
            Err(e) => {
                self.show_warning(format!("Failed to create new session: {e}"));
            }
        }
    }

    pub(super) fn handle_help_command(&mut self) {
        let commands = vec![
            "Available commands:",
            "  /help        - Show this help message",
            "  /new         - Create a new session",
            "  /name <name> - Rename current session",
            "  /session     - Show session info",
            "  /compact     - Trigger conversation compaction",
            "  /clear       - Clear chat display",
            "  /model       - Switch model",
            "  /hotkeys     - Show key bindings",
            "  /export      - Export session",
            "  /import      - Import session",
            "  /share       - Share session",
            "  /copy        - Copy last response",
            "  /debug       - Show debug info",
            "  /reload      - Reload resources",
            "  /quit        - Exit the application",
            "  !<cmd>       - Execute bash command",
            "  !!<cmd>      - Execute bash (excluded from context)",
        ];
        for line in commands {
            self.render_cache.chat_lines.push(line.to_string());
        }
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
                            self.render_cache.chat_lines.push(format!("Instructions: {inst}"));
                        }
                        self.show_status("Compaction prepared (async LLM summarization not wired in interactive mode yet)");
                    }
                    None => {
                        self.show_status(format!("Nothing to compact ({total_tokens} estimated tokens, {} entries)", entries.len()));
                    }
                }
            }
            Err(e) => {
                self.show_status(format!("Failed to get entries for compaction: {e}"));
            }
        }
        self.interaction.is_compacting = false;
    }

    pub(super) fn handle_reload_command(&mut self) {
        self.show_status("TODO: reload resources/extensions");
    }

    pub(super) fn handle_debug_command(&mut self) {
        self.show_status("TODO: debug command");
    }

    pub(super) fn handle_armin_says_hi(&mut self) {
        self.render_state_mut()
            .add_message_to_chat(InteractiveMessage::Assistant {
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
        self.ui.tui.show_overlay(overlay);
        self.show_status("Select session to resume");
    }

    pub(super) fn handle_resume_session(&mut self, session_id: &str) {
        // Switch the active session.
        self.session_setup.session_id = session_id.to_string();
        self.options.session_id = Some(session_id.to_string());
        let _ = self.controller.runtime_host.session_mut().clear_queue();

        // Clear all chat/streaming state (like /new).
        self.render_state_mut().chat_items.clear();
        self.render_state_mut().pending_items.clear();
        self.render_state_mut().streaming_component = None;
        self.streaming.streaming_text.clear();
        self.streaming.streaming_thinking.clear();
        self.streaming.streaming_tool_calls.clear();
        self.streaming.is_streaming = false;
        self.queues.steering_queue.clear();
        self.queues.follow_up_queue.clear();
        self.queues.compaction_queued_messages.clear();
        self.queues.pending_bash_components.clear();

        // Re-render session messages from the DB.
        if let Ok(rows) = store::get_entries(&self.session_setup.conn, session_id) {
            for row in rows {
                if let Ok(entry) = store::parse_entry(&row) {
                    match entry {
                        bb_core::types::SessionEntry::Message { message, .. } => match message {
                            bb_core::types::AgentMessage::User(u) => {
                                let text = u.content.iter().filter_map(|b| match b {
                                    bb_core::types::ContentBlock::Text { text } => Some(text.as_str()),
                                    _ => None,
                                }).collect::<Vec<_>>().join("\n");
                                self.render_state_mut().add_message_to_chat(
                                    super::super::events::InteractiveMessage::User { text },
                                );
                            }
                            bb_core::types::AgentMessage::Assistant(a) => {
                                use super::super::events::InteractiveMessage;
                                use super::super::components::assistant_message::{
                                    AssistantMessage as AMsg,
                                    AssistantMessageContent,
                                };
                                let mut content = Vec::new();
                                for c in &a.content {
                                    match c {
                                        bb_core::types::AssistantContent::Text { text } => {
                                            content.push(AssistantMessageContent::Text(text.clone()));
                                        }
                                        bb_core::types::AssistantContent::Thinking { thinking } => {
                                            content.push(AssistantMessageContent::Thinking(thinking.clone()));
                                        }
                                        _ => {}
                                    }
                                }
                                let msg = AMsg {
                                    content,
                                    stop_reason: None,
                                    error_message: a.error_message.clone(),
                                };
                                self.render_state_mut().add_message_to_chat(
                                    InteractiveMessage::Assistant {
                                        message: msg,
                                        tool_calls: vec![],
                                    },
                                );
                            }
                            _ => {}
                        },
                        _ => {}
                    }
                }
            }
        }

        self.rebuild_chat_container();
        self.rebuild_pending_container();
        self.rebuild_footer();

        self.render_state_mut().add_message_to_chat(
            super::super::events::InteractiveMessage::System {
                text: "Resumed session".to_string(),
            },
        );
        self.rebuild_chat_container();
        self.refresh_ui();
    }

    pub(super) fn handle_tree_navigate(&mut self, entry_id: &str) {
        let leaf_id = self.get_session_leaf().map(|id| id.0);
        if leaf_id.as_deref() == Some(entry_id) {
            self.show_status("Already at this point");
            return;
        }

        // Move the leaf pointer to the selected entry
        if let Err(e) = store::set_leaf(&self.session_setup.conn, &self.session_setup.session_id, Some(entry_id)) {
            self.show_warning(format!("Failed to navigate: {e}"));
            return;
        }

        // Clear and re-render from the new position
        self.render_state_mut().chat_items.clear();
        self.render_state_mut().pending_items.clear();
        self.render_state_mut().streaming_component = None;
        self.streaming.streaming_text.clear();
        self.streaming.streaming_thinking.clear();
        self.streaming.streaming_tool_calls.clear();
        self.streaming.is_streaming = false;
        self.queues.steering_queue.clear();
        self.queues.follow_up_queue.clear();

        // Re-render messages from root to new leaf
        if let Ok(path) = bb_session::tree::active_path(
            &self.session_setup.conn,
            &self.session_setup.session_id,
        ) {
            for row in &path {
                if let Ok(entry) = store::parse_entry(row) {
                    match entry {
                        bb_core::types::SessionEntry::Message { message, .. } => match message {
                            bb_core::types::AgentMessage::User(u) => {
                                let text: String = u.content.iter().filter_map(|b| match b {
                                    bb_core::types::ContentBlock::Text { text } => Some(text.as_str()),
                                    _ => None,
                                }).collect::<Vec<_>>().join("\n");
                                self.render_state_mut().add_message_to_chat(
                                    super::super::events::InteractiveMessage::User { text },
                                );
                            }
                            bb_core::types::AgentMessage::Assistant(a) => {
                                use super::super::components::assistant_message::{
                                    AssistantMessage as AMsg, AssistantMessageContent,
                                };
                                let mut content = Vec::new();
                                for c in &a.content {
                                    match c {
                                        bb_core::types::AssistantContent::Text { text } => {
                                            content.push(AssistantMessageContent::Text(text.clone()));
                                        }
                                        bb_core::types::AssistantContent::Thinking { thinking } => {
                                            content.push(AssistantMessageContent::Thinking(thinking.clone()));
                                        }
                                        _ => {}
                                    }
                                }
                                let msg = AMsg { content, stop_reason: None, error_message: a.error_message.clone() };
                                self.render_state_mut().add_message_to_chat(
                                    super::super::events::InteractiveMessage::Assistant {
                                        message: msg, tool_calls: vec![],
                                    },
                                );
                            }
                            _ => {}
                        },
                        _ => {}
                    }
                }
            }
        }

        self.rebuild_chat_container();
        self.rebuild_pending_container();
        self.rebuild_footer();

        self.render_state_mut().add_message_to_chat(
            super::super::events::InteractiveMessage::System {
                text: "Navigated to tree entry".to_string(),
            },
        );
        self.rebuild_chat_container();
        self.refresh_ui();
    }

    pub(super) fn shutdown(&mut self) {
        self.interaction.shutdown_requested = true;
        self.show_status("Shutdown requested");
    }

    pub(super) fn handle_bash_command(&mut self, command: &str, excluded_from_context: bool) {
        let label = if excluded_from_context { "bash(excluded)" } else { "bash" };
        self.render_cache.chat_lines.push(format!("{label}> {command}"));
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
                    self.render_cache.chat_lines.push(format!("exit code: {}", out.status.code().unwrap_or(-1)));
                }
            }
            Err(e) => {
                self.render_cache.chat_lines.push(format!("Failed to execute command: {e}"));
            }
        }
    }

    pub(super) fn flush_pending_bash_components(&mut self) {
        while let Some(line) = self.queues.pending_bash_components.pop_front() {
            self.render_cache.chat_lines.push(line);
        }
    }

    pub(super) fn is_extension_command(&self, text: &str) -> bool {
        text.starts_with("/ext") || text.starts_with("/extension")
    }

    pub(super) fn queue_compaction_message(&mut self, text: String, kind: QueuedMessageKind) {
        self.queues.compaction_queued_messages
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
            .unwrap_or_else(|| format!("{}/{}", self.session_setup.model.provider, self.session_setup.model.id));

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
        self.ui.tui.show_overlay(component);
        self.show_status("Opened model selector");
    }

    pub(super) fn show_placeholder(&mut self, label: &str) {
        self.show_status(format!("TODO: {label}"));
    }
}
