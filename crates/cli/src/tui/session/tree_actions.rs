use anyhow::{Result, anyhow};
use bb_core::types::{AgentMessage, SessionEntry};
use bb_session::{store, tree};
use bb_tui::select_list::SelectItem;
use bb_tui::tui::{TuiCommand, TuiSubmission};
use tokio::sync::mpsc::UnboundedReceiver;
use tokio_util::sync::CancellationToken;

use super::super::controller::TuiController;
use super::super::formatting::text_from_blocks;
use super::super::{FORK_ENTRY_MENU_ID, TREE_ENTRY_MENU_ID, TREE_SUMMARY_MENU_ID};

impl TuiController {
    pub(in crate::tui) fn open_tree_menu(&mut self, selected_entry_id: Option<&str>) -> Result<()> {
        let tree_nodes = tree::get_tree(&self.session_setup.conn, &self.session_setup.session_id)?;
        if tree_nodes.is_empty() {
            self.send_command(TuiCommand::SetStatusLine(
                "No entries in session".to_string(),
            ));
            return Ok(());
        }
        let entries = store::get_entries(&self.session_setup.conn, &self.session_setup.session_id)?;
        let leaf_id = store::get_session(&self.session_setup.conn, &self.session_setup.session_id)?
            .and_then(|row| row.leaf_id);

        self.send_command(TuiCommand::OpenTreeMenu {
            menu_id: TREE_ENTRY_MENU_ID.to_string(),
            title: "Session Tree".to_string(),
            tree: tree_nodes,
            entries,
            active_leaf: leaf_id,
            selected_value: selected_entry_id.map(str::to_string),
        });
        Ok(())
    }

    pub(in crate::tui) fn open_tree_summary_menu(&mut self, entry_id: &str) -> Result<()> {
        self.pending_tree_summary_target = Some(entry_id.to_string());
        self.pending_tree_custom_prompt_target = None;
        self.send_command(TuiCommand::OpenSelectMenu {
            menu_id: TREE_SUMMARY_MENU_ID.to_string(),
            title: "Branch summary".to_string(),
            items: vec![
                SelectItem {
                    label: "No summary".to_string(),
                    detail: Some("Jump directly to the selected point".to_string()),
                    value: "none".to_string(),
                },
                SelectItem {
                    label: "Summarize".to_string(),
                    detail: Some("Summarize abandoned branch context".to_string()),
                    value: "summarize".to_string(),
                },
                SelectItem {
                    label: "Summarize with custom prompt".to_string(),
                    detail: Some("Type custom branch-summary instructions".to_string()),
                    value: "custom".to_string(),
                },
            ],
            selected_value: None,
        });
        Ok(())
    }

    pub(in crate::tui) async fn handle_tree_summary_selection(
        &mut self,
        value: &str,
        submission_rx: &mut UnboundedReceiver<TuiSubmission>,
    ) -> Result<()> {
        let Some(target_entry_id) = self.pending_tree_summary_target.take() else {
            self.send_command(TuiCommand::SetStatusLine(
                "No tree target selected".to_string(),
            ));
            return Ok(());
        };

        match value {
            "none" => self.handle_tree_navigate(&target_entry_id),
            "summarize" => {
                self.summarize_tree_navigation(&target_entry_id, None, false, submission_rx)
                    .await
            }
            "custom" => {
                self.pending_tree_custom_prompt_target = Some(target_entry_id);
                self.send_command(TuiCommand::SetInput(String::new()));
                self.send_command(TuiCommand::SetStatusLine(
                    "Branch summary instructions (Enter submit, Esc/empty cancels)".to_string(),
                ));
                Ok(())
            }
            _ => {
                self.send_command(TuiCommand::SetStatusLine(format!(
                    "Unknown tree summary action: {value}"
                )));
                Ok(())
            }
        }
    }

    pub(in crate::tui) async fn summarize_tree_navigation(
        &mut self,
        entry_id: &str,
        instructions: Option<&str>,
        replace_instructions: bool,
        submission_rx: &mut UnboundedReceiver<TuiSubmission>,
    ) -> Result<()> {
        let cancel = CancellationToken::new();
        self.local_action_cancel = Some(cancel.clone());
        self.send_command(TuiCommand::SetStatusLine(
            "Summarizing branch... (Esc to cancel)".to_string(),
        ));

        let current_leaf_id = self.get_session_leaf().map(|id| id.0);
        let summary_mode = match instructions {
            Some(text) => crate::session_navigation::TreeSummaryMode::SummarizeCustom {
                instructions: text.to_string(),
                replace_instructions,
            },
            None => crate::session_navigation::TreeSummaryMode::Summarize,
        };

        enum TreeSummaryAction {
            Cancelled,
            Finished(crate::session_navigation::TreeNavigateOutcome),
            Closed,
        }

        let target_entry_id = entry_id.to_string();
        let action = {
            let navigate = crate::session_navigation::navigate_tree(
                &self.session_setup.conn,
                &self.session_setup.session_id,
                &target_entry_id,
                current_leaf_id.as_deref(),
                summary_mode,
                self.session_setup.provider.as_ref(),
                &self.session_setup.model.id,
                &self.session_setup.api_key,
                &self.session_setup.base_url,
                cancel.clone(),
            );
            tokio::pin!(navigate);

            loop {
                tokio::select! {
                    maybe_submission = submission_rx.recv() => {
                        match maybe_submission {
                            Some(TuiSubmission::CancelLocalAction) => {
                                cancel.cancel();
                                break TreeSummaryAction::Cancelled;
                            }
                            Some(TuiSubmission::Input(_))
                            | Some(TuiSubmission::InputWithImages { .. })
                            | Some(TuiSubmission::MenuSelection { .. })
                            | Some(TuiSubmission::ApprovalDecision { .. })
                            | Some(TuiSubmission::EditQueuedMessages) => {}
                            None => {
                                cancel.cancel();
                                break TreeSummaryAction::Closed;
                            }
                        }
                    }
                    outcome = &mut navigate => {
                        break TreeSummaryAction::Finished(outcome?);
                    }
                }
            }
        };
        self.local_action_cancel = None;

        match action {
            TreeSummaryAction::Cancelled => {
                self.send_command(TuiCommand::SetInput(String::new()));
                self.send_command(TuiCommand::SetStatusLine(
                    "Tree navigation cancelled".to_string(),
                ));
                self.open_tree_summary_menu(&target_entry_id)?;
                Ok(())
            }
            TreeSummaryAction::Finished(outcome) => {
                self.rebuild_current_transcript()?;
                self.publish_footer();
                self.send_command(TuiCommand::SetInput(
                    outcome.editor_text.unwrap_or_default(),
                ));
                self.send_command(TuiCommand::SetStatusLine(
                    if outcome.summary_entry_id.is_some() {
                        "Summarized branch and navigated".to_string()
                    } else {
                        "Navigated to selected point".to_string()
                    },
                ));
                Ok(())
            }
            TreeSummaryAction::Closed => Ok(()),
        }
    }

    pub(in crate::tui) fn handle_tree_navigate(&mut self, entry_id: &str) -> Result<()> {
        store::set_leaf(
            &self.session_setup.conn,
            &self.session_setup.session_id,
            Some(entry_id),
        )?;
        self.rebuild_current_transcript()?;
        self.publish_footer();
        self.send_command(TuiCommand::SetStatusLine(
            "Navigated to selected point".to_string(),
        ));
        Ok(())
    }

    pub(in crate::tui) fn open_fork_menu(&mut self) -> Result<()> {
        let rows = store::get_entries(&self.session_setup.conn, &self.session_setup.session_id)?;
        let items: Vec<SelectItem> = rows
            .into_iter()
            .filter_map(|row| {
                let entry = store::parse_entry(&row).ok()?;
                match entry {
                    SessionEntry::Message {
                        base,
                        message: AgentMessage::User(user),
                        ..
                    } => {
                        let text = text_from_blocks(&user.content, " ")
                            .trim()
                            .replace('\n', " ");
                        if text.is_empty() {
                            None
                        } else {
                            Some(SelectItem {
                                label: text.clone(),
                                detail: None,
                                value: base.id.0,
                            })
                        }
                    }
                    _ => None,
                }
            })
            .collect();
        if items.is_empty() {
            self.send_command(TuiCommand::SetStatusLine(
                "No messages to fork from".to_string(),
            ));
            return Ok(());
        }
        self.send_command(TuiCommand::OpenSelectMenu {
            menu_id: FORK_ENTRY_MENU_ID.to_string(),
            title: "Select a user message to fork from".to_string(),
            items,
            selected_value: None,
        });
        Ok(())
    }

    pub(in crate::tui) fn handle_fork_from_entry(&mut self, entry_id: &str) -> Result<()> {
        let row = store::get_entry(
            &self.session_setup.conn,
            &self.session_setup.session_id,
            entry_id,
        )?
        .ok_or_else(|| anyhow!("Entry not found"))?;
        let entry = store::parse_entry(&row)?;
        let editor_text = match entry {
            SessionEntry::Message {
                message: AgentMessage::User(user),
                ..
            } => text_from_blocks(&user.content, "\n"),
            _ => String::new(),
        };
        store::set_leaf(
            &self.session_setup.conn,
            &self.session_setup.session_id,
            row.parent_id.as_deref(),
        )?;
        self.rebuild_current_transcript()?;
        self.publish_footer();
        self.send_command(TuiCommand::SetInput(editor_text));
        self.send_command(TuiCommand::SetStatusLine(
            "Forked — edit and send to create a new branch".to_string(),
        ));
        Ok(())
    }
}
