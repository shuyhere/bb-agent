use anyhow::Result;
use bb_core::agent_session::ThinkingLevel;
use bb_core::types::{AgentMessage, ContentBlock, EntryBase, EntryId, SessionEntry, UserMessage};
use bb_session::store;
use bb_tui::tui::TuiCommand;
use chrono::Utc;

use super::super::controller::{PendingImage, TuiController};
use super::{HIDDEN_DISPATCH_PREFIX, build_tui_transcript};

impl TuiController {
    pub(in crate::tui) fn ensure_session_row_created(&mut self) -> Result<()> {
        if self.session_setup.session_created {
            return Ok(());
        }

        let cwd = self.session_setup.tool_ctx.cwd.display().to_string();
        store::create_session_with_id(
            &self.session_setup.conn,
            &self.session_setup.session_id,
            &cwd,
        )?;
        // Persist the initial thinking level so later resume can distinguish an
        // explicit session setting from the absence of any recorded override.
        let initial_thinking = ThinkingLevel::parse(&self.session_setup.thinking_level)
            .unwrap_or(ThinkingLevel::Medium);
        self.append_thinking_level_change_entry(initial_thinking)?;
        self.session_setup.session_created = true;
        Ok(())
    }

    pub(in crate::tui) fn append_thinking_level_change_entry(
        &mut self,
        thinking_level: ThinkingLevel,
    ) -> Result<()> {
        let entry = SessionEntry::ThinkingLevelChange {
            base: EntryBase {
                id: EntryId::generate(),
                parent_id: self.get_session_leaf(),
                timestamp: Utc::now(),
            },
            thinking_level,
        };
        store::append_entry(
            &self.session_setup.conn,
            &self.session_setup.session_id,
            &entry,
        )?;
        Ok(())
    }

    pub(in crate::tui) fn append_user_entry_to_db_with_images(
        &mut self,
        prompt: &str,
        images: &[PendingImage],
    ) -> Result<()> {
        let mut content = vec![ContentBlock::Text {
            text: prompt.to_string(),
        }];
        for img in images {
            content.push(ContentBlock::Image {
                data: img.data.clone(),
                mime_type: img.mime_type.clone(),
            });
        }

        let user_entry = SessionEntry::Message {
            base: EntryBase {
                id: EntryId::generate(),
                parent_id: self.get_session_leaf(),
                timestamp: Utc::now(),
            },
            message: AgentMessage::User(UserMessage {
                content,
                timestamp: Utc::now().timestamp_millis(),
            }),
        };

        store::append_entry(
            &self.session_setup.conn,
            &self.session_setup.session_id,
            &user_entry,
        )?;
        Ok(())
    }

    pub(in crate::tui) fn append_hidden_user_entry(&mut self, prompt: &str) -> Result<()> {
        let user_entry = SessionEntry::Message {
            base: EntryBase {
                id: EntryId::generate(),
                parent_id: self.get_session_leaf(),
                timestamp: Utc::now(),
            },
            message: AgentMessage::User(UserMessage {
                content: vec![ContentBlock::Text {
                    text: format!("{HIDDEN_DISPATCH_PREFIX}{prompt}"),
                }],
                timestamp: Utc::now().timestamp_millis(),
            }),
        };

        store::append_entry(
            &self.session_setup.conn,
            &self.session_setup.session_id,
            &user_entry,
        )?;
        Ok(())
    }

    pub(in crate::tui) fn auto_name_session(&mut self, prompt: &str) {
        let session_row =
            store::get_session(&self.session_setup.conn, &self.session_setup.session_id)
                .ok()
                .flatten();
        if session_row
            .as_ref()
            .and_then(|row| row.name.as_deref())
            .is_some()
        {
            return;
        }

        let name = prompt.trim().replace('\n', " ");
        let name = if name.chars().count() > 80 {
            let truncated: String = name.chars().take(77).collect();
            format!("{truncated}...")
        } else {
            name
        };

        let _ = store::set_session_name(
            &self.session_setup.conn,
            &self.session_setup.session_id,
            Some(&name),
        );
    }

    pub(in crate::tui) fn rebuild_current_transcript(&mut self) -> Result<()> {
        let (transcript, tool_states) =
            build_tui_transcript(&self.session_setup.conn, &self.session_setup.session_id)?;
        self.send_command(TuiCommand::SetTranscriptWithToolStates {
            transcript,
            tool_states,
        });
        Ok(())
    }

    pub(in crate::tui) fn get_session_leaf(&self) -> Option<EntryId> {
        crate::turn_runner::get_leaf_raw(&self.session_setup.conn, &self.session_setup.session_id)
    }
}
