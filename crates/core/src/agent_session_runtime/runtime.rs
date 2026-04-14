mod compaction;
mod retry_bash;
mod tree;

use serde::{Deserialize, Serialize};

use super::session_tree::SessionTreeState;
use super::types::*;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct AgentSessionRuntime {
    pub model: Option<RuntimeModelRef>,
    pub messages: Vec<RuntimeMessage>,
    pub session_tree: SessionTreeState,
    pub compaction_state: CompactionState,
    pub retry_state: RetryState,
    pub bash_state: BashExecutionState,
    pub tree_state: TreeNavigationState,
    pub queued_continue_requested: bool,
    pub emitted_events: Vec<RuntimeEvent>,
}

impl AgentSessionRuntime {
    fn emit(&mut self, event: RuntimeEvent) {
        self.emitted_events.push(event);
    }

    fn remove_last_assistant_message(&mut self) {
        if matches!(self.messages.last(), Some(RuntimeMessage::Assistant(_))) {
            self.messages.pop();
        }
    }

    fn resolve_retry(&mut self) {
        self.retry_state.in_progress = false;
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;

    fn user_message(text: &str) -> RuntimeMessage {
        RuntimeMessage::User {
            content: vec![MessagePart::Text {
                text: text.to_string(),
            }],
            timestamp: Utc::now(),
        }
    }

    fn custom_message(text: &str) -> SessionTreeEntryKind {
        SessionTreeEntryKind::CustomMessage {
            content: vec![MessagePart::Text {
                text: text.to_string(),
            }],
        }
    }

    #[test]
    fn navigate_tree_emits_summary_source_for_extension_summary() {
        let mut runtime = AgentSessionRuntime {
            model: Some(RuntimeModelRef {
                provider: "test".to_string(),
                id: "demo".to_string(),
                context_window: 8_192,
            }),
            ..AgentSessionRuntime::default()
        };
        let root_id = runtime.session_tree.append_entry(
            None,
            SessionTreeEntryKind::Message(user_message("draft prompt")),
        );
        runtime
            .session_tree
            .append_entry(Some(root_id.clone()), custom_message("assistant scratch"));
        runtime.messages = runtime.session_tree.build_session_context_messages();

        let outcome = runtime
            .navigate_tree(
                &root_id,
                NavigateTreeOptions::default(),
                Some("summarized branch".to_string()),
                Some(serde_json::json!({"kind": "test"})),
                RuntimeEntrySource::Extension,
            )
            .expect("navigate tree");

        let summary_entry = outcome.summary_entry.expect("summary entry");
        match &summary_entry.kind {
            SessionTreeEntryKind::BranchSummary(entry) => {
                assert!(entry.from_extension);
                assert_eq!(entry.source(), RuntimeEntrySource::Extension);
            }
            other => panic!("expected branch summary, got {other:?}"),
        }
        assert_eq!(outcome.editor_text.as_deref(), Some("draft prompt"));
        assert!(matches!(
            runtime.emitted_events.last(),
            Some(RuntimeEvent::SessionTree {
                summary_source: Some(RuntimeEntrySource::Extension),
                ..
            })
        ));
    }

    #[test]
    fn finish_compaction_stores_runtime_entry_source() {
        let mut runtime = AgentSessionRuntime::default();
        let result = CompactionResult {
            summary: "compact summary".to_string(),
            first_kept_entry_id: "entry-1".to_string(),
            tokens_before: 42,
            details: Some(serde_json::json!({"source": "test"})),
        };

        runtime.finish_compaction(
            CompactionReason::Manual,
            result,
            RuntimeEntrySource::Runtime,
        );

        let entry = runtime
            .session_tree
            .entries()
            .last()
            .expect("compaction entry should exist");
        match &entry.kind {
            SessionTreeEntryKind::Compaction(entry) => {
                assert!(!entry.from_extension);
                assert_eq!(entry.source(), RuntimeEntrySource::Runtime);
            }
            other => panic!("expected compaction entry, got {other:?}"),
        }
    }
}
