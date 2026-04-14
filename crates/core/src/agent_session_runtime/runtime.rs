mod compaction;
mod retry_bash;
mod tree;

use serde::{Deserialize, Serialize};

use super::session_tree::SessionTreeState;
use super::types::*;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct AgentSessionRuntime {
    pub(in crate::agent_session_runtime) model: Option<RuntimeModelRef>,
    pub(in crate::agent_session_runtime) messages: Vec<RuntimeMessage>,
    pub(in crate::agent_session_runtime) session_tree: SessionTreeState,
    pub(in crate::agent_session_runtime) compaction_state: CompactionState,
    pub(in crate::agent_session_runtime) retry_state: RetryState,
    pub(in crate::agent_session_runtime) bash_state: BashExecutionState,
    pub(in crate::agent_session_runtime) tree_state: TreeNavigationState,
    pub(in crate::agent_session_runtime) queued_continue_requested: bool,
    pub(in crate::agent_session_runtime) emitted_events: Vec<RuntimeEvent>,
}

impl AgentSessionRuntime {
    pub(in crate::agent_session_runtime) fn with_model(model: Option<RuntimeModelRef>) -> Self {
        Self {
            model,
            ..Self::default()
        }
    }

    pub fn set_model(&mut self, model: Option<RuntimeModelRef>) {
        self.model = model;
    }

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

    fn assistant_message(
        stop_reason: AssistantStopReason,
        total_context_tokens: usize,
    ) -> AssistantMessage {
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

    fn assistant_error_message(
        error_message: &str,
        total_context_tokens: usize,
    ) -> AssistantMessage {
        AssistantMessage {
            error_message: Some(error_message.to_string()),
            ..assistant_message(AssistantStopReason::Error, total_context_tokens)
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

        runtime.fail_compaction(CompactionReason::Manual, "cancelled".to_string(), true);

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

    #[test]
    fn is_retryable_error_distinguishes_transient_failures_from_context_overflow() {
        let runtime = runtime_with_model(100);
        assert!(
            runtime
                .is_retryable_error(&assistant_error_message("provider returned error 503", 40,))
        );
        assert!(!runtime.is_retryable_error(&assistant_error_message(
            "maximum context length exceeded",
            100,
        )));
    }

    #[test]
    fn handle_retryable_error_schedules_retry_and_removes_last_assistant_message() {
        let mut runtime = runtime_with_model(8_192);
        runtime.messages.push(user_message("before retry"));
        let assistant = assistant_error_message("provider returned error 503", 512);
        runtime
            .messages
            .push(RuntimeMessage::Assistant(assistant.clone()));

        let action = runtime.handle_retryable_error(
            &assistant,
            &RetrySettings {
                enabled: true,
                max_retries: 3,
                base_delay_ms: 250,
            },
        );

        assert_eq!(
            action,
            RetryAction::Scheduled {
                attempt: 1,
                delay_ms: 250,
            }
        );
        assert_eq!(runtime.retry_state.attempt, 1);
        assert!(runtime.retry_state.in_progress);
        assert!(matches!(
            runtime.messages.last(),
            Some(RuntimeMessage::User { .. })
        ));
        assert!(matches!(
            runtime.emitted_events.last(),
            Some(RuntimeEvent::AutoRetryStart {
                attempt: 1,
                max_attempts: 3,
                delay_ms: 250,
                ..
            })
        ));
    }

    #[test]
    fn complete_retry_cycle_clears_retry_state_and_emits_failure_details() {
        let mut runtime = AgentSessionRuntime::default();
        runtime.retry_state.attempt = 2;
        runtime.retry_state.in_progress = true;
        runtime.retry_state.abort_requested = true;

        runtime.complete_retry_cycle(RetryCompletion::Failed {
            final_error: Some("still overloaded".to_string()),
        });

        assert_eq!(runtime.retry_state, RetryState::default());
        assert!(matches!(
            runtime.emitted_events.last(),
            Some(RuntimeEvent::AutoRetryEnd {
                success: false,
                attempt: 2,
                final_error: Some(error),
            }) if error == "still overloaded"
        ));
    }

    #[test]
    fn streamed_bash_results_are_buffered_until_flush() {
        let mut runtime = AgentSessionRuntime::default();
        let prepared = runtime.prepare_bash_command(
            "echo hi",
            "/tmp/demo",
            Some("set -e"),
            BashContextPolicy::Exclude,
        );

        assert_eq!(prepared.resolved_command, "set -e\necho hi");
        assert_eq!(prepared.cwd, "/tmp/demo");
        assert!(runtime.is_bash_running());

        runtime.record_bash_result(
            prepared,
            BashResult {
                output: "hi".to_string(),
                exit_code: 0,
                cancelled: false,
                truncated: false,
                full_output_path: None,
            },
            BashMessageDelivery::StreamPending,
        );

        assert!(!runtime.is_bash_running());
        assert!(runtime.has_pending_bash_messages());
        assert!(runtime.messages.is_empty());

        runtime.flush_pending_bash_messages();

        match runtime.messages.last() {
            Some(RuntimeMessage::BashExecution(message)) => {
                assert_eq!(message.command, "echo hi");
                assert_eq!(message.output, "hi");
                assert!(message.exclude_from_context);
            }
            other => panic!("expected buffered bash execution message, got {other:?}"),
        }
        assert!(!runtime.has_pending_bash_messages());
    }

    #[test]
    fn compact_manual_reports_missing_model_and_already_compacted_states() {
        let mut no_model = AgentSessionRuntime::default();
        assert!(matches!(
            no_model.compact_manual(&compaction_settings(), None),
            Err(AgentSessionRuntimeError::NoModelSelected)
        ));

        let mut already_compacted = runtime_with_model(100);
        append_user_entry(&mut already_compacted, "first");
        append_user_entry(&mut already_compacted, "second");
        already_compacted.messages = already_compacted
            .session_tree
            .build_session_context_messages();
        already_compacted.finish_compaction(
            CompactionReason::Manual,
            CompactionResult {
                summary: "summary".to_string(),
                first_kept_entry_id: "entry-2".to_string(),
                tokens_before: 10,
                details: None,
            },
            RuntimeEntrySource::Runtime,
        );

        assert!(matches!(
            already_compacted.compact_manual(
                &CompactionSettings {
                    min_entries: 10,
                    ..compaction_settings()
                },
                None,
            ),
            Err(AgentSessionRuntimeError::AlreadyCompacted)
        ));
    }

    #[test]
    fn compact_manual_reports_nothing_to_compact_for_short_branch() {
        let mut runtime = runtime_with_model(100);
        append_user_entry(&mut runtime, "only one entry");
        runtime.messages = runtime.session_tree.build_session_context_messages();

        assert!(matches!(
            runtime.compact_manual(&compaction_settings(), None),
            Err(AgentSessionRuntimeError::NothingToCompact)
        ));
    }

    #[test]
    fn abort_helpers_update_retry_bash_and_tree_state() {
        let mut runtime = AgentSessionRuntime::default();
        runtime.retry_state.attempt = 2;
        runtime.retry_state.in_progress = true;
        runtime.abort_retry();
        assert_eq!(
            runtime.retry_state,
            RetryState {
                attempt: 0,
                in_progress: false,
                abort_requested: true,
            }
        );
        assert!(matches!(
            runtime.emitted_events.last(),
            Some(RuntimeEvent::AutoRetryEnd {
                success: false,
                attempt: 2,
                final_error: Some(error),
            }) if error == "Retry cancelled"
        ));

        runtime.abort_bash();
        assert!(runtime.bash_state.abort_requested);

        runtime.abort_compaction();
        assert!(runtime.compaction_state.abort_requested);

        runtime.abort_branch_summary();
        assert!(runtime.tree_state.abort_requested);
    }

    #[test]
    fn reset_overflow_recovery_and_collect_user_messages_for_forking() {
        let mut runtime = runtime_with_model(100);
        append_user_entry(&mut runtime, "first");
        runtime.session_tree.append_entry(
            runtime.session_tree.leaf_id().map(ToOwned::to_owned),
            SessionTreeEntryKind::Message(user_message("")),
        );
        runtime.session_tree.append_entry(
            runtime.session_tree.leaf_id().map(ToOwned::to_owned),
            custom_message("custom"),
        );
        append_user_entry(&mut runtime, "second");

        let user_messages = runtime.get_user_messages_for_forking();
        assert_eq!(user_messages.len(), 2);
        assert_eq!(user_messages[0].1, "first");
        assert_eq!(user_messages[1].1, "second");

        runtime.compaction_state.overflow_recovery_attempted = true;
        runtime.reset_overflow_recovery();
        assert!(!runtime.compaction_state.overflow_recovery_attempted);
    }
}
