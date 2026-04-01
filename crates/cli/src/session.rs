//! AgentSession — manages the full lifecycle of a coding agent session.

use crate::agent_loop;
use bb_core::agent_loop::{AgentLoopEvent, ContextUsage};
use bb_core::types::*;
use anyhow::Result;
use bb_provider::registry::Model;
use bb_provider::Provider;
use bb_session::store;
use bb_tools::{Tool, ToolContext};
use chrono::Utc;
use tokio::sync::mpsc;

/// The AgentSession manages the full lifecycle of a coding session.
pub struct AgentSession {
    pub conn: rusqlite::Connection,
    pub session_id: String,
    pub system_prompt: String,
    pub model: Model,
    pub provider: Box<dyn Provider>,
    pub api_key: String,
    pub base_url: String,
    pub tools: Vec<Box<dyn Tool>>,
    pub tool_defs: Vec<serde_json::Value>,
    pub tool_ctx: ToolContext,
    pub compaction_settings: CompactionSettings,
}

impl AgentSession {
    /// Run a single user prompt through the full agent loop.
    ///
    /// Appends the user message to the session, then runs the agent loop
    /// which streams `AgentLoopEvent`s to `event_tx`. The caller should
    /// receive events and display them appropriately.
    pub async fn run_prompt(
        &self,
        prompt: &str,
        event_tx: mpsc::UnboundedSender<AgentLoopEvent>,
    ) -> Result<()> {
        // Append user message to session
        let user_entry = SessionEntry::Message {
            base: EntryBase {
                id: EntryId::generate(),
                parent_id: self.get_leaf(),
                timestamp: Utc::now(),
            },
            message: AgentMessage::User(UserMessage {
                content: vec![ContentBlock::Text {
                    text: prompt.to_string(),
                }],
                timestamp: Utc::now().timestamp_millis(),
            }),
        };
        store::append_entry(&self.conn, &self.session_id, &user_entry)?;

        // Run the agent loop
        agent_loop::run_agent_loop(
            &self.conn,
            &self.session_id,
            &self.system_prompt,
            &self.model,
            &*self.provider,
            &self.api_key,
            &self.base_url,
            &self.tools,
            &self.tool_defs,
            &self.tool_ctx,
            &event_tx,
        )
        .await
    }

    /// Get current context usage (estimated).
    pub fn context_usage(&self) -> Option<ContextUsage> {
        let ctx = bb_session::context::build_context(&self.conn, &self.session_id).ok()?;
        // Rough estimation: ~4 chars per token
        let estimated_tokens: u64 = ctx
            .messages
            .iter()
            .map(|m| {
                let char_count: usize = match m {
                    AgentMessage::User(u) => u
                        .content
                        .iter()
                        .map(|c| match c {
                            ContentBlock::Text { text } => text.len(),
                            ContentBlock::Image { data, .. } => data.len(),
                        })
                        .sum(),
                    AgentMessage::Assistant(a) => a
                        .content
                        .iter()
                        .map(|c| match c {
                            AssistantContent::Text { text } => text.len(),
                            AssistantContent::Thinking { thinking } => thinking.len(),
                            AssistantContent::ToolCall { arguments, .. } => {
                                arguments.to_string().len()
                            }
                        })
                        .sum(),
                    AgentMessage::ToolResult(t) => t
                        .content
                        .iter()
                        .map(|c| match c {
                            ContentBlock::Text { text } => text.len(),
                            ContentBlock::Image { data, .. } => data.len(),
                        })
                        .sum(),
                    AgentMessage::CompactionSummary(c) => c.summary.len(),
                    AgentMessage::BranchSummary(b) => b.summary.len(),
                    _ => 0,
                };
                (char_count / 4) as u64
            })
            .sum();

        let context_window = self.model.context_window;
        let percent = if context_window > 0 {
            (estimated_tokens as f64 / context_window as f64) * 100.0
        } else {
            0.0
        };

        Some(ContextUsage {
            tokens: estimated_tokens,
            context_window,
            percent,
        })
    }

    /// Trigger manual compaction (stub — full implementation in Sprint 2).
    pub async fn compact(&self, _instructions: Option<&str>) -> Result<()> {
        tracing::warn!("Compaction not yet implemented");
        Ok(())
    }

    /// Check if auto-compaction should trigger, and run it if so.
    /// Returns true if compaction was performed.
    pub async fn maybe_auto_compact(&self) -> Result<bool> {
        if !self.compaction_settings.enabled {
            return Ok(false);
        }

        if let Some(usage) = self.context_usage() {
            let threshold = self.model.context_window - self.compaction_settings.reserve_tokens;
            if usage.tokens > threshold {
                self.compact(None).await?;
                return Ok(true);
            }
        }

        Ok(false)
    }

    /// Get the current leaf entry ID.
    fn get_leaf(&self) -> Option<EntryId> {
        store::get_session(&self.conn, &self.session_id)
            .ok()
            .flatten()
            .and_then(|s| s.leaf_id.map(EntryId))
    }
}

/// Build tool definitions from a list of tools (for the provider).
pub fn build_tool_defs(tools: &[Box<dyn Tool>]) -> Vec<serde_json::Value> {
    tools
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
        .collect()
}
