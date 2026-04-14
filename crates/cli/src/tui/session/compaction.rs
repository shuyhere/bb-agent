use anyhow::{Result, anyhow};
use bb_session::{compaction, store};
use bb_tui::tui::{TuiCommand, TuiNoteLevel, TuiSubmission};
use tokio::sync::mpsc::UnboundedReceiver;
use tokio_util::sync::CancellationToken;

use super::super::controller::{ManualCompactionEvent, QueuedPrompt, TuiController};

impl TuiController {
    pub(in crate::tui) async fn handle_compact_command(
        &mut self,
        instructions: Option<&str>,
    ) -> Result<()> {
        if self.streaming || self.manual_compaction_in_progress {
            self.queued_prompts.push_back(match instructions {
                Some(instructions) => QueuedPrompt::Visible(format!("/compact {instructions}")),
                None => QueuedPrompt::Visible("/compact".to_string()),
            });
            self.publish_status();
            return Ok(());
        }

        let settings = bb_core::types::CompactionSettings {
            enabled: self.session_setup.compaction_enabled,
            reserve_tokens: self.session_setup.compaction_reserve_tokens,
            keep_recent_tokens: self.session_setup.compaction_keep_recent_tokens,
        };

        let cancel = CancellationToken::new();
        self.local_action_cancel = Some(cancel.clone());
        self.manual_compaction_in_progress = true;
        self.manual_compaction_generation += 1;
        let generation = self.manual_compaction_generation;
        self.send_command(TuiCommand::SetLocalActionActive(true));
        self.send_command(TuiCommand::SetStatusLine(
            "Compacting session... (Esc to cancel)".to_string(),
        ));
        self.publish_status();
        self.publish_footer();

        let entries = store::get_entries(&self.session_setup.conn, &self.session_setup.session_id)?;
        let parent_id = crate::turn_runner::get_leaf_raw(
            &self.session_setup.conn,
            &self.session_setup.session_id,
        );
        let db_path = self
            .session_setup
            .conn
            .path()
            .map(std::path::PathBuf::from)
            .ok_or_else(|| anyhow!("Compaction requires a file-backed session database"))?;
        let session_id = self.session_setup.session_id.clone();
        let provider = self.session_setup.provider.clone();
        let model_id = self.session_setup.model.id.clone();
        let api_key = self.session_setup.api_key.clone();
        let base_url = self.session_setup.base_url.clone();
        let headers = self.session_setup.headers.clone();
        let manual_compaction_tx = self.manual_compaction_tx.clone();
        let instructions = instructions.map(str::to_string);

        tokio::spawn(async move {
            let result = crate::compaction_exec::execute_session_compaction(
                entries,
                parent_id,
                db_path,
                &session_id,
                provider,
                &model_id,
                &api_key,
                &base_url,
                &headers,
                &settings,
                instructions.as_deref(),
                cancel,
            )
            .await;
            let _ =
                manual_compaction_tx.send(ManualCompactionEvent::Finished { generation, result });
        });
        Ok(())
    }

    pub(in crate::tui) async fn handle_manual_compaction_event(
        &mut self,
        event: ManualCompactionEvent,
        submission_rx: &mut UnboundedReceiver<TuiSubmission>,
    ) -> Result<()> {
        let ManualCompactionEvent::Finished { generation, result } = event;
        if generation != self.manual_compaction_generation {
            return Ok(());
        }

        self.local_action_cancel = None;
        self.manual_compaction_in_progress = false;
        self.send_command(TuiCommand::SetLocalActionActive(false));

        match result {
            Ok(result) => {
                self.rebuild_current_transcript()?;
                self.publish_footer();
                self.send_command(TuiCommand::SetStatusLine(format!(
                    "Compaction complete • {} messages summarized • {} kept • {} tokens before",
                    result.summarized_count, result.kept_count, result.tokens_before
                )));
            }
            Err(err) if err.to_string() == "Nothing to compact" => {
                let entries =
                    store::get_entries(&self.session_setup.conn, &self.session_setup.session_id)?;
                let total_tokens: u64 = entries.iter().map(compaction::estimate_tokens_row).sum();
                self.publish_footer();
                self.send_command(TuiCommand::PushNote {
                    level: TuiNoteLevel::Status,
                    text: format!(
                        "Nothing to compact ({total_tokens} estimated tokens, {} entries)",
                        entries.len()
                    ),
                });
                self.send_command(TuiCommand::SetStatusLine("Nothing to compact".to_string()));
            }
            Err(err) if err.to_string().to_ascii_lowercase().contains("cancel") => {
                self.publish_footer();
                self.send_command(TuiCommand::SetStatusLine(
                    "Compaction cancelled".to_string(),
                ));
            }
            Err(err) => {
                self.publish_footer();
                self.send_command(TuiCommand::PushNote {
                    level: TuiNoteLevel::Error,
                    text: format!("Compaction failed: {err}"),
                });
                self.send_command(TuiCommand::SetStatusLine("Compaction failed".to_string()));
            }
        }

        if !self.queued_prompts.is_empty() {
            self.drain_queued_prompts(submission_rx).await?;
        }
        Ok(())
    }
}
