use anyhow::Result;
use bb_core::types::*;
use rusqlite::Connection;

use crate::store::{self, EntryRow};
use crate::tree;

/// Build the session context (what gets sent to the LLM).
///
/// Walks root → leaf, applies compaction boundary, returns messages.
pub fn build_context(conn: &Connection, session_id: &str) -> Result<SessionContext> {
    let path = tree::active_path(conn, session_id)?;
    build_context_from_path(&path)
}

/// Build context from a pre-computed path (for testing / reuse).
pub fn build_context_from_path(path: &[EntryRow]) -> Result<SessionContext> {
    let mut messages = Vec::new();
    let mut model: Option<ModelInfo> = None;
    let mut thinking_level = ThinkingLevel::Off;

    if path.is_empty() {
        return Ok(SessionContext {
            messages,
            thinking_level,
            model,
        });
    }

    // Parse all entries
    let entries: Vec<SessionEntry> = path
        .iter()
        .map(|row| store::parse_entry(row))
        .collect::<Result<Vec<_>>>()?;

    // Find last compaction on path
    let compaction_idx = entries
        .iter()
        .enumerate()
        .rev()
        .find(|(_, e)| matches!(e, SessionEntry::Compaction { .. }))
        .map(|(i, _)| i);

    if let Some(comp_idx) = compaction_idx {
        let comp = &entries[comp_idx];

        // 1. Emit compaction summary
        if let SessionEntry::Compaction {
            summary,
            tokens_before,
            first_kept_entry_id,
            ..
        } = comp
        {
            messages.push(AgentMessage::CompactionSummary(CompactionSummaryMessage {
                summary: summary.clone(),
                tokens_before: *tokens_before,
                timestamp: comp.base().timestamp.timestamp_millis(),
            }));

            // 2. Emit kept messages before compaction
            let first_kept = first_kept_entry_id.as_str();
            let mut found = false;
            for entry in &entries[..comp_idx] {
                if entry.base().id.as_str() == first_kept {
                    found = true;
                }
                if found {
                    append_message(&mut messages, entry);
                }
                update_settings(entry, &mut model, &mut thinking_level);
            }

            // 3. Emit messages after compaction
            for entry in &entries[comp_idx + 1..] {
                append_message(&mut messages, entry);
                update_settings(entry, &mut model, &mut thinking_level);
            }
        }
    } else {
        // No compaction — emit all messages
        for entry in &entries {
            append_message(&mut messages, entry);
            update_settings(entry, &mut model, &mut thinking_level);
        }
    }

    Ok(SessionContext {
        messages,
        thinking_level,
        model,
    })
}

fn append_message(messages: &mut Vec<AgentMessage>, entry: &SessionEntry) {
    match entry {
        SessionEntry::Message { message, .. } => {
            messages.push(message.clone());
        }
        SessionEntry::BranchSummary {
            summary, from_id, base, ..
        } => {
            messages.push(AgentMessage::BranchSummary(BranchSummaryMessage {
                summary: summary.clone(),
                from_id: from_id.as_str().to_string(),
                timestamp: base.timestamp.timestamp_millis(),
            }));
        }
        SessionEntry::CustomMessage {
            custom_type,
            content,
            display,
            details,
            base,
            ..
        } => {
            messages.push(AgentMessage::Custom(CustomMessage {
                custom_type: custom_type.clone(),
                content: content.clone(),
                display: *display,
                details: details.clone(),
                timestamp: base.timestamp.timestamp_millis(),
            }));
        }
        // Other entry types don't produce LLM messages
        _ => {}
    }
}

fn update_settings(
    entry: &SessionEntry,
    model: &mut Option<ModelInfo>,
    thinking_level: &mut ThinkingLevel,
) {
    match entry {
        SessionEntry::ModelChange {
            provider, model_id, ..
        } => {
            *model = Some(ModelInfo {
                provider: provider.clone(),
                model_id: model_id.clone(),
            });
        }
        SessionEntry::ThinkingLevelChange {
            thinking_level: level,
            ..
        } => {
            *thinking_level = match level.as_str() {
                "off" => ThinkingLevel::Off,
                "minimal" => ThinkingLevel::Minimal,
                "low" => ThinkingLevel::Low,
                "medium" => ThinkingLevel::Medium,
                "high" => ThinkingLevel::High,
                _ => ThinkingLevel::Off,
            };
        }
        SessionEntry::Message {
            message: AgentMessage::Assistant(asst),
            ..
        } => {
            *model = Some(ModelInfo {
                provider: asst.provider.clone(),
                model_id: asst.model.clone(),
            });
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store;
    use chrono::Utc;

    #[test]
    fn test_build_context_empty() {
        let conn = store::open_memory().unwrap();
        let sid = store::create_session(&conn, "/tmp").unwrap();
        let ctx = build_context(&conn, &sid).unwrap();
        assert!(ctx.messages.is_empty());
    }

    #[test]
    fn test_build_context_simple() {
        let conn = store::open_memory().unwrap();
        let sid = store::create_session(&conn, "/tmp").unwrap();

        let e1 = SessionEntry::Message {
            base: EntryBase {
                id: EntryId::generate(),
                parent_id: None,
                timestamp: Utc::now(),
            },
            message: AgentMessage::User(UserMessage {
                content: vec![ContentBlock::Text {
                    text: "hello".into(),
                }],
                timestamp: Utc::now().timestamp_millis(),
            }),
        };
        store::append_entry(&conn, &sid, &e1).unwrap();

        let ctx = build_context(&conn, &sid).unwrap();
        assert_eq!(ctx.messages.len(), 1);
        assert!(matches!(ctx.messages[0], AgentMessage::User(_)));
    }

    #[test]
    fn test_build_context_with_compaction() {
        let conn = store::open_memory().unwrap();
        let sid = store::create_session(&conn, "/tmp").unwrap();

        // e1: user (will be summarized)
        let e1 = SessionEntry::Message {
            base: EntryBase {
                id: EntryId("e1000001".into()),
                parent_id: None,
                timestamp: Utc::now(),
            },
            message: AgentMessage::User(UserMessage {
                content: vec![ContentBlock::Text { text: "old msg".into() }],
                timestamp: 1000,
            }),
        };
        store::append_entry(&conn, &sid, &e1).unwrap();

        // e2: user (kept after compaction)
        let e2 = SessionEntry::Message {
            base: EntryBase {
                id: EntryId("e2000002".into()),
                parent_id: Some(EntryId("e1000001".into())),
                timestamp: Utc::now(),
            },
            message: AgentMessage::User(UserMessage {
                content: vec![ContentBlock::Text { text: "kept msg".into() }],
                timestamp: 2000,
            }),
        };
        store::append_entry(&conn, &sid, &e2).unwrap();

        // e3: compaction
        let e3 = SessionEntry::Compaction {
            base: EntryBase {
                id: EntryId("e3000003".into()),
                parent_id: Some(EntryId("e2000002".into())),
                timestamp: Utc::now(),
            },
            summary: "Summary of old conversation".into(),
            first_kept_entry_id: EntryId("e2000002".into()),
            tokens_before: 5000,
            details: None,
            from_plugin: false,
        };
        store::append_entry(&conn, &sid, &e3).unwrap();

        // e4: user (after compaction)
        let e4 = SessionEntry::Message {
            base: EntryBase {
                id: EntryId("e4000004".into()),
                parent_id: Some(EntryId("e3000003".into())),
                timestamp: Utc::now(),
            },
            message: AgentMessage::User(UserMessage {
                content: vec![ContentBlock::Text { text: "new msg".into() }],
                timestamp: 4000,
            }),
        };
        store::append_entry(&conn, &sid, &e4).unwrap();

        let ctx = build_context(&conn, &sid).unwrap();

        // Should have: summary + kept msg (e2) + new msg (e4) = 3 messages
        assert_eq!(ctx.messages.len(), 3);
        assert!(matches!(ctx.messages[0], AgentMessage::CompactionSummary(_)));
        assert!(matches!(ctx.messages[1], AgentMessage::User(_)));
        assert!(matches!(ctx.messages[2], AgentMessage::User(_)));

        // e1 should NOT be in context (summarized away)
        if let AgentMessage::User(u) = &ctx.messages[1] {
            assert_eq!(
                u.content[0],
                ContentBlock::Text { text: "kept msg".into() }
            );
        }
    }
}
