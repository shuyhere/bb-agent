use std::sync::Arc;

use anyhow::{Result, anyhow};
use bb_core::types::{CompactionSettings, EntryBase, EntryId, SessionEntry};
use bb_provider::Provider;
use bb_session::store::EntryRow;
use chrono::Utc;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone)]
pub(crate) struct ExecutedCompaction {
    pub tokens_before: u64,
    pub summarized_count: usize,
    pub kept_count: usize,
}

pub(crate) async fn execute_session_compaction(
    entries: Vec<EntryRow>,
    parent_id: Option<EntryId>,
    db_path: std::path::PathBuf,
    session_id: &str,
    provider: Arc<dyn Provider>,
    model_id: &str,
    api_key: &str,
    base_url: &str,
    headers: &std::collections::HashMap<String, String>,
    settings: &CompactionSettings,
    custom_instructions: Option<&str>,
    cancel: CancellationToken,
) -> Result<ExecutedCompaction> {
    let prep = bb_session::compaction::prepare_compaction(&entries, settings)
        .ok_or_else(|| anyhow!("Nothing to compact"))?;

    let summarized_count = prep.messages_to_summarize.len();
    let kept_count = prep.kept_messages.len();

    let result = bb_session::compaction::compact(bb_session::compaction::CompactionRequest {
        preparation: &prep,
        provider: provider.as_ref(),
        model: model_id,
        api_key,
        base_url,
        headers,
        custom_instructions,
        cancel,
    })
    .await?;

    let details = serde_json::json!({
        "summarizedCount": summarized_count,
        "keptCount": kept_count,
        "readFiles": result.read_files,
        "modifiedFiles": result.modified_files,
    });

    let compaction_entry = SessionEntry::Compaction {
        base: EntryBase {
            id: EntryId::generate(),
            parent_id,
            timestamp: Utc::now(),
        },
        summary: result.summary.clone(),
        first_kept_entry_id: EntryId(result.first_kept_entry_id.clone()),
        tokens_before: result.tokens_before,
        details: Some(details),
        from_plugin: false,
    };

    let append_conn = bb_session::store::open_db(&db_path)?;
    bb_session::store::append_entry(&append_conn, session_id, &compaction_entry)?;

    Ok(ExecutedCompaction {
        tokens_before: result.tokens_before,
        summarized_count,
        kept_count,
    })
}
