use anyhow::Result;
use bb_core::types::{EntryBase, EntryId, SessionEntry};
use bb_provider::Provider;
use chrono::Utc;
use rusqlite::Connection;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeSummaryMode {
    Summarize,
    SummarizeCustom {
        instructions: String,
        replace_instructions: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeNavigateOutcome {
    pub editor_text: Option<String>,
    pub new_leaf_id: Option<String>,
    pub summary_entry_id: Option<String>,
}

#[allow(clippy::too_many_arguments)]
pub async fn navigate_tree(
    conn: &Connection,
    session_id: &str,
    target_entry_id: &str,
    current_leaf_id: Option<&str>,
    summary_mode: TreeSummaryMode,
    provider: &dyn Provider,
    model: &str,
    api_key: &str,
    base_url: &str,
    cancel: CancellationToken,
) -> Result<TreeNavigateOutcome> {
    match summary_mode {
        TreeSummaryMode::Summarize => {
            navigate_tree_with_summary_impl(
                conn,
                session_id,
                target_entry_id,
                current_leaf_id,
                None,
                false,
                provider,
                model,
                api_key,
                base_url,
                cancel,
            )
            .await
        }
        TreeSummaryMode::SummarizeCustom {
            instructions,
            replace_instructions,
        } => {
            navigate_tree_with_summary_impl(
                conn,
                session_id,
                target_entry_id,
                current_leaf_id,
                Some(instructions.as_str()),
                replace_instructions,
                provider,
                model,
                api_key,
                base_url,
                cancel,
            )
            .await
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn navigate_tree_with_summary_impl(
    conn: &Connection,
    session_id: &str,
    target_entry_id: &str,
    current_leaf_id: Option<&str>,
    custom_instructions: Option<&str>,
    replace_instructions: bool,
    provider: &dyn Provider,
    model: &str,
    api_key: &str,
    base_url: &str,
    cancel: CancellationToken,
) -> Result<TreeNavigateOutcome> {
    let resolved = bb_session::tree::resolve_tree_target(conn, session_id, target_entry_id)?;
    let collected = bb_session::tree::collect_entries_for_branch_summary(
        conn,
        session_id,
        current_leaf_id,
        target_entry_id,
    )?;

    if collected.is_empty() {
        let resolved = bb_session::tree::resolve_tree_target(conn, session_id, target_entry_id)?;
        let new_leaf_id = resolved.new_leaf_id().map(ToOwned::to_owned);
        match new_leaf_id.as_deref() {
            Some(new_leaf_id) => bb_session::store::set_leaf(conn, session_id, Some(new_leaf_id))?,
            None => bb_session::store::set_leaf(conn, session_id, None)?,
        }
        return Ok(TreeNavigateOutcome {
            editor_text: resolved.into_editor_text(),
            new_leaf_id,
            summary_entry_id: None,
        });
    }

    let result = bb_session::branch_summary::generate_branch_summary(
        bb_session::branch_summary::BranchSummaryRequest {
            rows: collected.entries(),
            provider,
            model,
            api_key,
            base_url,
            custom_instructions,
            replace_instructions,
            cancel,
        },
    )
    .await?;

    let new_leaf_id = resolved.new_leaf_id().map(ToOwned::to_owned);
    let editor_text = resolved.editor_text().map(ToOwned::to_owned);
    match new_leaf_id.as_deref() {
        Some(new_leaf_id) => bb_session::store::set_leaf(conn, session_id, Some(new_leaf_id))?,
        None => bb_session::store::set_leaf(conn, session_id, None)?,
    }

    let summary_entry = SessionEntry::BranchSummary {
        base: EntryBase {
            id: EntryId::generate(),
            parent_id: new_leaf_id.clone().map(EntryId),
            timestamp: Utc::now(),
        },
        from_id: EntryId(current_leaf_id.unwrap_or("root").to_string()),
        summary: result.summary,
        details: Some(serde_json::json!({
            "read_files": result.read_files,
            "modified_files": result.modified_files,
        })),
        from_plugin: false,
    };
    let summary_entry_id = summary_entry.base().id.0.clone();
    bb_session::store::append_entry(conn, session_id, &summary_entry)?;

    Ok(TreeNavigateOutcome {
        editor_text,
        new_leaf_id: Some(summary_entry_id.clone()),
        summary_entry_id: Some(summary_entry_id),
    })
}
