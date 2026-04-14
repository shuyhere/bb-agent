use super::*;
use crate::agent_session_runtime::algorithms::{
    estimate_context_tokens, get_latest_compaction_entry, is_context_overflow, prepare_compaction,
    should_compact,
};

impl AgentSessionRuntime {
    pub fn compact_manual(
        &mut self,
        settings: &CompactionSettings,
        custom_instructions: Option<String>,
    ) -> Result<CompactionPreparation, AgentSessionRuntimeError> {
        let _ = custom_instructions;
        self.compaction_state.manual_in_progress = true;
        self.compaction_state.abort_requested = false;
        self.emit(RuntimeEvent::CompactionStart {
            reason: CompactionReason::Manual,
        });

        if self.model.is_none() {
            self.compaction_state.manual_in_progress = false;
            return Err(AgentSessionRuntimeError::NoModelSelected);
        }

        let branch = self.session_tree.get_branch();
        let preparation = prepare_compaction(&branch, settings).ok_or_else(|| {
            if matches!(
                branch.last().map(|entry| &entry.kind),
                Some(SessionTreeEntryKind::Compaction(_))
            ) {
                AgentSessionRuntimeError::AlreadyCompacted
            } else {
                AgentSessionRuntimeError::NothingToCompact
            }
        })?;

        Ok(preparation)
    }

    pub fn finish_compaction(
        &mut self,
        reason: CompactionReason,
        result: CompactionResult,
        source: RuntimeEntrySource,
    ) {
        self.session_tree.append_compaction(
            result.summary.clone(),
            result.first_kept_entry_id.clone(),
            result.tokens_before,
            result.details.clone(),
            source,
        );
        self.messages = self.session_tree.build_session_context_messages();
        self.compaction_state.manual_in_progress = false;
        self.compaction_state.auto_in_progress = false;
        self.compaction_state.abort_requested = false;
        self.emit(RuntimeEvent::CompactionEnd {
            reason,
            result: Some(result),
            aborted: false,
            will_retry: matches!(reason, CompactionReason::Overflow),
            error_message: None,
        });
        if matches!(reason, CompactionReason::Overflow) {
            self.queued_continue_requested = true;
        }
    }

    pub fn fail_compaction(
        &mut self,
        reason: CompactionReason,
        error_message: String,
        aborted: bool,
    ) {
        self.compaction_state.manual_in_progress = false;
        self.compaction_state.auto_in_progress = false;
        self.compaction_state.abort_requested = false;
        self.emit(RuntimeEvent::CompactionEnd {
            reason,
            result: None,
            aborted,
            will_retry: false,
            error_message: if aborted { None } else { Some(error_message) },
        });
    }

    pub fn abort_compaction(&mut self) {
        self.compaction_state.abort_requested = true;
    }

    /// Evaluates whether the current assistant message should trigger an automatic compaction.
    ///
    /// By default aborted assistant messages are ignored because they do not represent a stable
    /// provider-side context reading. Tests and future recovery flows can opt into considering
    /// aborted messages explicitly through `CompactionCheckOptions`.
    pub fn check_compaction(
        &mut self,
        assistant_message: &AssistantMessage,
        settings: &CompactionSettings,
        options: CompactionCheckOptions,
    ) -> CompactionAction {
        if !settings.enabled {
            return CompactionAction::None;
        }

        if options.should_ignore_aborted_message()
            && assistant_message.stop_reason == AssistantStopReason::Aborted
        {
            return CompactionAction::None;
        }

        let context_window = self
            .model
            .as_ref()
            .map(|model| model.context_window)
            .unwrap_or_default();
        let same_model = self
            .model
            .as_ref()
            .map(|model| {
                assistant_message.provider.as_deref() == Some(model.provider.as_str())
                    && assistant_message.model.as_deref() == Some(model.id.as_str())
            })
            .unwrap_or(false);

        let branch = self.session_tree.get_branch();
        let latest_compaction = get_latest_compaction_entry(&branch);
        let assistant_is_before_compaction = latest_compaction
            .map(|entry| assistant_message.timestamp <= entry.timestamp)
            .unwrap_or(false);
        if assistant_is_before_compaction {
            return CompactionAction::None;
        }

        if same_model && is_context_overflow(assistant_message, context_window) {
            if self.compaction_state.overflow_recovery_attempted {
                let message = "Context overflow recovery failed after one compact-and-retry attempt. Try reducing context or switching to a larger-context model.".to_string();
                self.emit(RuntimeEvent::CompactionEnd {
                    reason: CompactionReason::Overflow,
                    result: None,
                    aborted: false,
                    will_retry: false,
                    error_message: Some(message.clone()),
                });
                return CompactionAction::OverflowRecoveryFailed { message };
            }

            let preparation = prepare_compaction(&branch, settings);
            self.compaction_state.overflow_recovery_attempted = true;
            self.remove_last_assistant_message();
            if let Some(preparation) = preparation {
                self.compaction_state.auto_in_progress = true;
                self.emit(RuntimeEvent::CompactionStart {
                    reason: CompactionReason::Overflow,
                });
                return CompactionAction::RecoverOverflow { preparation };
            }
            return CompactionAction::None;
        }

        let context_tokens = if assistant_message.stop_reason == AssistantStopReason::Error {
            let estimate = estimate_context_tokens(&self.messages);
            let Some(last_usage_index) = estimate.last_usage_index else {
                return CompactionAction::None;
            };
            if let Some(compaction_entry) = latest_compaction
                && let Some(RuntimeMessage::Assistant(usage_message)) =
                    self.messages.get(last_usage_index)
                && usage_message.timestamp <= compaction_entry.timestamp
            {
                return CompactionAction::None;
            }
            estimate.tokens
        } else {
            assistant_message.usage.total_context_tokens()
        };

        if should_compact(context_tokens, context_window, settings)
            && let Some(preparation) = prepare_compaction(&branch, settings)
        {
            self.compaction_state.auto_in_progress = true;
            self.emit(RuntimeEvent::CompactionStart {
                reason: CompactionReason::Threshold,
            });
            return CompactionAction::CompactForThreshold { preparation };
        }

        CompactionAction::None
    }

    pub fn reset_overflow_recovery(&mut self) {
        self.compaction_state.overflow_recovery_attempted = false;
    }
}
