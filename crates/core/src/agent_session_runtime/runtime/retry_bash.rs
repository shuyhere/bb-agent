use super::*;
use crate::agent_session_runtime::algorithms::is_context_overflow;
use chrono::Utc;

impl AgentSessionRuntime {
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
}
