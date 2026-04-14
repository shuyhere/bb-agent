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

    fn runtime_with_model(context_window: usize) -> AgentSessionRuntime {
        AgentSessionRuntime {
            model: Some(RuntimeModelRef {
                provider: "test".to_string(),
                id: "demo".to_string(),
                context_window,
            }),
            ..AgentSessionRuntime::default()
        }
    }

    fn append_user_entry(runtime: &mut AgentSessionRuntime, text: &str) -> String {
        runtime.session_tree.append_entry(
            runtime.session_tree.leaf_id().map(ToOwned::to_owned),
            SessionTreeEntryKind::Message(user_message(text)),
        )
    }

    fn assistant_message(stop_reason: AssistantStopReason, total_context_tokens: usize) -> AssistantMessage {
        AssistantMessage {
            content: vec![MessagePart::Text {
                text: "assistant".to_string(),
            }],
            timestamp: Utc::now(),
            stop_reason,
            usage: RuntimeUsage {
                input: total_context_tokens,
                output: 0,
                cache_read: 0,
                cache_write: 0,
                cost: RuntimeCost::default(),
            },
            error_message: None,
            provider: Some("test".to_string()),
            model: Some("demo".to_string()),
        }
    }

    fn compaction_settings() -> CompactionSettings {
        CompactionSettings {
            enabled: true,
            threshold_percent: 80,
            reserve_tokens: 0,
            min_entries: 2,
            keep_recent_entries: 1,
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

    #[test]
    fn check_compaction_ignores_aborted_messages_by_default() {
        let mut runtime = runtime_with_model(100);
        append_user_entry(&mut runtime, "first");
        append_user_entry(&mut runtime, "second");
        runtime.messages = runtime.session_tree.build_session_context_messages();

        let action = runtime.check_compaction(
            &assistant_message(AssistantStopReason::Aborted, 95),
            &compaction_settings(),
            CompactionCheckOptions::default(),
        );

        assert_eq!(action, CompactionAction::None);
        assert!(!runtime.compaction_state.auto_in_progress);
        assert!(runtime.emitted_events.is_empty());
    }

    #[test]
    fn check_compaction_can_consider_aborted_messages_explicitly() {
        let mut runtime = runtime_with_model(100);
        append_user_entry(&mut runtime, "first");
        append_user_entry(&mut runtime, "second");
        runtime.messages = runtime.session_tree.build_session_context_messages();

        let action = runtime.check_compaction(
            &assistant_message(AssistantStopReason::Aborted, 95),
            &compaction_settings(),
            CompactionCheckOptions {
                aborted_message_behavior: AbortedMessageBehavior::Consider,
            },
        );

        match action {
            CompactionAction::CompactForThreshold { preparation } => {
                assert_eq!(preparation.entries_to_summarize, 1);
            }
            other => panic!("expected threshold compaction, got {other:?}"),
        }
        assert!(runtime.compaction_state.auto_in_progress);
        assert!(matches!(
            runtime.emitted_events.last(),
            Some(RuntimeEvent::CompactionStart {
                reason: CompactionReason::Threshold,
            })
        ));
    }

    #[test]
    fn fail_compaction_clears_active_state_and_reports_abort() {
        let mut runtime = AgentSessionRuntime::default();
        runtime.compaction_state.manual_in_progress = true;
        runtime.compaction_state.auto_in_progress = true;
        runtime.compaction_state.abort_requested = true;

        runtime.fail_compaction(
            CompactionReason::Manual,
            "cancelled".to_string(),
            true,
        );

        assert_eq!(
            runtime.compaction_state,
            CompactionState {
                manual_in_progress: false,
                auto_in_progress: false,
                overflow_recovery_attempted: false,
                abort_requested: false,
            }
        );
        assert!(matches!(
            runtime.emitted_events.last(),
            Some(RuntimeEvent::CompactionEnd {
                reason: CompactionReason::Manual,
                aborted: true,
                will_retry: false,
                error_message: None,
                ..
            })
        ));
    }
}
