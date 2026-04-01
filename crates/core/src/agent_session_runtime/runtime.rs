use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::algorithms::{
    collect_entries_for_branch_summary, estimate_context_tokens, extract_text,
    get_latest_compaction_entry, is_context_overflow, prepare_compaction, should_compact,
};
use super::session_tree::SessionTreeState;
use super::types::*;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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

impl Default for AgentSessionRuntime {
    fn default() -> Self {
        Self {
            model: None,
            messages: Vec::new(),
            session_tree: SessionTreeState::default(),
            compaction_state: CompactionState::default(),
            retry_state: RetryState::default(),
            bash_state: BashExecutionState::default(),
            tree_state: TreeNavigationState::default(),
            queued_continue_requested: false,
            emitted_events: Vec::new(),
        }
    }
}

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
        from_extension: bool,
    ) {
        self.session_tree.append_compaction(
            result.summary.clone(),
            result.first_kept_entry_id.clone(),
            result.tokens_before,
            result.details.clone(),
            from_extension,
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

    pub fn check_compaction(
        &mut self,
        assistant_message: &AssistantMessage,
        settings: &CompactionSettings,
        skip_aborted_check: bool,
    ) -> CompactionAction {
        if !settings.enabled {
            return CompactionAction::None;
        }

        if skip_aborted_check && assistant_message.stop_reason == AssistantStopReason::Aborted {
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
            if let Some(compaction_entry) = latest_compaction {
                if let Some(RuntimeMessage::Assistant(usage_message)) =
                    self.messages.get(last_usage_index)
                {
                    if usage_message.timestamp <= compaction_entry.timestamp {
                        return CompactionAction::None;
                    }
                }
            }
            estimate.tokens
        } else {
            assistant_message.usage.total_context_tokens()
        };

        if should_compact(context_tokens, context_window, settings) {
            if let Some(preparation) = prepare_compaction(&branch, settings) {
                self.compaction_state.auto_in_progress = true;
                self.emit(RuntimeEvent::CompactionStart {
                    reason: CompactionReason::Threshold,
                });
                return CompactionAction::CompactForThreshold { preparation };
            }
        }

        CompactionAction::None
    }

    pub fn reset_overflow_recovery(&mut self) {
        self.compaction_state.overflow_recovery_attempted = false;
    }

    pub fn is_retryable_error(&self, message: &AssistantMessage) -> bool {
        if message.stop_reason != AssistantStopReason::Error {
            return false;
        }

        let Some(error_message) = message.error_message.as_deref() else {
            return false;
        };

        let context_window = self
            .model
            .as_ref()
            .map(|model| model.context_window)
            .unwrap_or_default();
        if is_context_overflow(message, context_window) {
            return false;
        }

        let err = error_message.to_ascii_lowercase();
        [
            "overloaded",
            "provider returned error",
            "rate limit",
            "too many requests",
            "429",
            "500",
            "502",
            "503",
            "504",
            "service unavailable",
            "server error",
            "internal error",
            "network error",
            "connection error",
            "connection refused",
            "other side closed",
            "fetch failed",
            "upstream connect",
            "reset before headers",
            "socket hang up",
            "timed out",
            "timeout",
            "terminated",
            "retry delay",
        ]
        .iter()
        .any(|needle| err.contains(needle))
    }

    pub fn handle_retryable_error(
        &mut self,
        message: &AssistantMessage,
        settings: &RetrySettings,
    ) -> RetryAction {
        if !settings.enabled {
            self.resolve_retry();
            return RetryAction::Disabled;
        }

        self.retry_state.in_progress = true;
        self.retry_state.abort_requested = false;
        self.retry_state.attempt += 1;

        if self.retry_state.attempt > settings.max_retries {
            let attempts = self.retry_state.attempt.saturating_sub(1);
            let final_error = message.error_message.clone();
            self.emit(RuntimeEvent::AutoRetryEnd {
                success: false,
                attempt: attempts,
                final_error: final_error.clone(),
            });
            self.retry_state.attempt = 0;
            self.retry_state.in_progress = false;
            self.resolve_retry();
            return RetryAction::MaxRetriesExceeded {
                attempts,
                final_error,
            };
        }

        let delay_ms = settings
            .base_delay_ms
            .saturating_mul(2u64.saturating_pow(self.retry_state.attempt - 1));
        self.emit(RuntimeEvent::AutoRetryStart {
            attempt: self.retry_state.attempt,
            max_attempts: settings.max_retries,
            delay_ms,
            error_message: message
                .error_message
                .clone()
                .unwrap_or_else(|| "Unknown error".to_string()),
        });
        self.remove_last_assistant_message();

        RetryAction::Scheduled {
            attempt: self.retry_state.attempt,
            delay_ms,
        }
    }

    pub fn complete_retry_cycle(&mut self, success: bool, final_error: Option<String>) {
        let attempt = self.retry_state.attempt;
        self.emit(RuntimeEvent::AutoRetryEnd {
            success,
            attempt,
            final_error,
        });
        self.retry_state.attempt = 0;
        self.retry_state.in_progress = false;
        self.retry_state.abort_requested = false;
        self.resolve_retry();
    }

    pub fn abort_retry(&mut self) {
        self.retry_state.abort_requested = true;
        let attempt = self.retry_state.attempt;
        self.retry_state.attempt = 0;
        self.retry_state.in_progress = false;
        self.emit(RuntimeEvent::AutoRetryEnd {
            success: false,
            attempt,
            final_error: Some("Retry cancelled".to_string()),
        });
        self.resolve_retry();
    }

    pub fn prepare_bash_command(
        &mut self,
        command: impl Into<String>,
        cwd: impl Into<String>,
        shell_command_prefix: Option<&str>,
        exclude_from_context: bool,
    ) -> PreparedBashCommand {
        let original_command = command.into();
        let resolved_command = shell_command_prefix
            .filter(|prefix| !prefix.is_empty())
            .map(|prefix| format!("{prefix}\n{original_command}"))
            .unwrap_or_else(|| original_command.clone());
        self.bash_state.running_command = Some(original_command.clone());
        self.bash_state.abort_requested = false;
        PreparedBashCommand {
            original_command,
            resolved_command,
            cwd: cwd.into(),
            exclude_from_context,
        }
    }

    pub fn record_bash_result(
        &mut self,
        command: impl Into<String>,
        result: BashResult,
        exclude_from_context: bool,
        streaming: bool,
    ) {
        let bash_message = BashExecutionMessage {
            command: command.into(),
            output: result.output,
            exit_code: result.exit_code,
            cancelled: result.cancelled,
            truncated: result.truncated,
            full_output_path: result.full_output_path,
            timestamp: Utc::now(),
            exclude_from_context,
        };

        if streaming {
            self.bash_state.pending_messages.push(bash_message);
        } else {
            self.messages
                .push(RuntimeMessage::BashExecution(bash_message));
        }
        self.bash_state.running_command = None;
        self.bash_state.abort_requested = false;
    }

    pub fn abort_bash(&mut self) {
        self.bash_state.abort_requested = true;
    }

    pub fn is_bash_running(&self) -> bool {
        self.bash_state.running_command.is_some()
    }

    pub fn has_pending_bash_messages(&self) -> bool {
        !self.bash_state.pending_messages.is_empty()
    }

    pub fn flush_pending_bash_messages(&mut self) {
        for message in self.bash_state.pending_messages.drain(..) {
            self.messages.push(RuntimeMessage::BashExecution(message));
        }
    }

    pub fn navigate_tree(
        &mut self,
        target_id: &str,
        options: NavigateTreeOptions,
        summary_text: Option<String>,
        summary_details: Option<Value>,
        from_extension: bool,
    ) -> Result<NavigateTreeOutcome, AgentSessionRuntimeError> {
        let old_leaf_id = self.session_tree.leaf_id().map(ToOwned::to_owned);

        if old_leaf_id.as_deref() == Some(target_id) {
            return Ok(NavigateTreeOutcome {
                editor_text: None,
                cancelled: false,
                aborted: false,
                summary_entry: None,
            });
        }

        if options.summarize && self.model.is_none() {
            return Err(AgentSessionRuntimeError::NoModelForSummarization);
        }

        let target_entry = self
            .session_tree
            .get_entry(target_id)
            .cloned()
            .ok_or_else(|| AgentSessionRuntimeError::EntryNotFound(target_id.to_string()))?;

        let collected = collect_entries_for_branch_summary(
            &self.session_tree,
            old_leaf_id.as_deref(),
            target_id,
        );
        let _preparation = TreePreparation {
            target_id: target_id.to_string(),
            old_leaf_id: old_leaf_id.clone(),
            common_ancestor_id: collected.common_ancestor_id.clone(),
            entries_to_summarize: collected
                .entries
                .iter()
                .map(|entry| entry.id.clone())
                .collect(),
            user_wants_summary: options.summarize,
            custom_instructions: options.custom_instructions.clone(),
            replace_instructions: options.replace_instructions,
            label: options.label.clone(),
        };

        self.tree_state.branch_summary_in_progress = options.summarize;
        self.tree_state.abort_requested = false;

        let (new_leaf_id, editor_text) = match &target_entry.kind {
            SessionTreeEntryKind::Message(RuntimeMessage::User { content, .. }) => {
                (target_entry.parent_id.clone(), Some(extract_text(content)))
            }
            SessionTreeEntryKind::CustomMessage { content } => {
                (target_entry.parent_id.clone(), Some(extract_text(content)))
            }
            _ => (Some(target_entry.id.clone()), None),
        };

        let summary_entry = if let Some(summary) = summary_text {
            let summary_id = self.session_tree.branch_with_summary(
                new_leaf_id.clone(),
                summary,
                summary_details,
                from_extension,
            );
            if let Some(label) = options.label {
                self.session_tree
                    .append_label_change(summary_id.clone(), label);
            }
            self.session_tree.get_entry(&summary_id).cloned()
        } else {
            match new_leaf_id.clone() {
                None => self.session_tree.reset_leaf(),
                Some(id) => self.session_tree.branch(id),
            }
            if let Some(label) = options.label {
                self.session_tree
                    .append_label_change(target_id.to_string(), label);
            }
            None
        };

        self.messages = self.session_tree.build_session_context_messages();
        self.tree_state.branch_summary_in_progress = false;
        self.tree_state.abort_requested = false;
        self.emit(RuntimeEvent::SessionTree {
            new_leaf_id: self.session_tree.leaf_id().map(ToOwned::to_owned),
            old_leaf_id,
            summary_entry_id: summary_entry.as_ref().map(|entry| entry.id.clone()),
            from_extension: summary_entry.as_ref().map(|_| from_extension),
        });

        Ok(NavigateTreeOutcome {
            editor_text,
            cancelled: false,
            aborted: false,
            summary_entry,
        })
    }

    pub fn abort_branch_summary(&mut self) {
        self.tree_state.abort_requested = true;
    }

    pub fn get_user_messages_for_forking(&self) -> Vec<(String, String)> {
        self.session_tree
            .entries()
            .iter()
            .filter_map(|entry| match &entry.kind {
                SessionTreeEntryKind::Message(RuntimeMessage::User { content, .. }) => {
                    Some((entry.id.clone(), extract_text(content)))
                }
                _ => None,
            })
            .filter(|(_, text)| !text.is_empty())
            .collect()
    }

    pub fn get_context_usage(&self) -> Option<ContextUsage> {
        let model = self.model.as_ref()?;
        if model.context_window == 0 {
            return None;
        }

        let branch = self.session_tree.get_branch();
        let latest_compaction = get_latest_compaction_entry(&branch);
        if let Some(compaction_entry) = latest_compaction {
            let branch_ids: Vec<&str> = branch.iter().map(|entry| entry.id.as_str()).collect();
            let compaction_index = branch_ids
                .iter()
                .rposition(|id| *id == compaction_entry.id)?;
            let mut has_post_compaction_usage = false;
            for entry in branch.iter().skip(compaction_index + 1).rev() {
                if let SessionTreeEntryKind::Message(RuntimeMessage::Assistant(message)) =
                    &entry.kind
                {
                    if message.stop_reason != AssistantStopReason::Aborted
                        && message.stop_reason != AssistantStopReason::Error
                        && message.usage.total_context_tokens() > 0
                    {
                        has_post_compaction_usage = true;
                        break;
                    }
                }
            }
            if !has_post_compaction_usage {
                return Some(ContextUsage {
                    tokens: None,
                    context_window: model.context_window,
                    percent: None,
                });
            }
        }

        let estimate = estimate_context_tokens(&self.messages);
        let percent =
            ((estimate.tokens as f64 / model.context_window as f64) * 100.0).round() as u8;
        Some(ContextUsage {
            tokens: Some(estimate.tokens),
            context_window: model.context_window,
            percent: Some(percent),
        })
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

