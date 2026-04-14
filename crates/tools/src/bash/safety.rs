use serde_json::{Map, Value, json};

use crate::bash_policy::BashSafetyAssessment;
use crate::sandbox::{SandboxBackend, SandboxFailureDetails};
use crate::support::text_result_with;
use crate::{ExecutionPolicy, ToolApprovalRequest, ToolContext, ToolExecutionMode, ToolResult};

#[derive(Clone, Copy)]
pub(super) struct BashSafetyContext<'a> {
    pub safety: &'a BashSafetyAssessment,
    pub approval_required: bool,
    pub approved: bool,
    pub execution_policy: ExecutionPolicy,
}

impl BashSafetyContext<'_> {
    fn to_value(self) -> Value {
        json!({
            "executionPolicy": self.execution_policy.as_str(),
            "approvalRequired": self.approval_required,
            "approved": self.approved,
            "title": self.safety.title,
            "reason": self.safety.reason,
        })
    }
}

#[derive(Clone, Copy)]
pub(super) struct BashResultDetails<'a> {
    pub command: &'a str,
    pub exit_code: Option<i32>,
    pub cancelled: bool,
    pub timed_out: bool,
    pub truncated: bool,
    pub safety: BashSafetyContext<'a>,
    pub sandbox_backend: Option<SandboxBackend>,
    pub sandbox_failure: Option<&'a SandboxFailureDetails>,
}

impl<'a> BashResultDetails<'a> {
    pub(super) fn error(
        command: &'a str,
        safety: BashSafetyContext<'a>,
        sandbox_backend: Option<SandboxBackend>,
        sandbox_failure: Option<&'a SandboxFailureDetails>,
    ) -> Self {
        Self {
            command,
            exit_code: None,
            cancelled: false,
            timed_out: false,
            truncated: false,
            safety,
            sandbox_backend,
            sandbox_failure,
        }
    }
}

pub(super) async fn request_bash_approval(
    command: &str,
    ctx: &ToolContext,
    safety: &BashSafetyAssessment,
) -> Result<bool, ToolResult> {
    let approval_required = ctx.execution_policy == ExecutionPolicy::Safety
        && matches!(
            safety.disposition,
            crate::bash_policy::BashSafetyDisposition::ApprovalRequired
        );

    if !approval_required {
        return Ok(false);
    }

    match ctx.execution_mode {
        ToolExecutionMode::NonInteractive => Err(approval_denied_result(
            command,
            safety,
            "Command requires approval in interactive mode and cannot run in non-interactive mode",
            ctx.execution_policy,
        )),
        ToolExecutionMode::Interactive => {
            let Some(request_approval) = ctx.request_approval.as_ref() else {
                return Err(approval_denied_result(
                    command,
                    safety,
                    "Interactive approval UI is unavailable for this command",
                    ctx.execution_policy,
                ));
            };
            let outcome = request_approval(ToolApprovalRequest {
                tool_name: "bash".to_string(),
                title: safety.title.clone(),
                command: command.to_string(),
                reason: safety.reason.clone(),
            })
            .await;
            if !outcome.approved() {
                return Err(approval_denied_result(
                    command,
                    safety,
                    "Command was denied by the interactive approval flow",
                    ctx.execution_policy,
                ));
            }
            Ok(true)
        }
    }
}

pub(super) fn build_details(details: BashResultDetails<'_>) -> Value {
    let mut value = Map::from_iter([
        ("command".to_string(), Value::from(details.command)),
        (
            "exitCode".to_string(),
            details.exit_code.map(Value::from).unwrap_or(Value::Null),
        ),
        ("cancelled".to_string(), Value::from(details.cancelled)),
        ("timedOut".to_string(), Value::from(details.timed_out)),
        ("truncated".to_string(), Value::from(details.truncated)),
        ("safety".to_string(), details.safety.to_value()),
    ]);

    if let Some(backend) = details.sandbox_backend {
        let mut sandbox = Map::from_iter([
            ("mode".to_string(), Value::from("safety")),
            ("backend".to_string(), backend_detail(backend)),
        ]);
        if let Some(failure) = details.sandbox_failure {
            sandbox.insert("failure".to_string(), failure.to_value());
        }
        value.insert("sandbox".to_string(), Value::Object(sandbox));
    }

    Value::Object(value)
}

pub(super) fn structured_error_result(
    message: String,
    details: BashResultDetails<'_>,
) -> ToolResult {
    text_result_with(message, Some(build_details(details)), true, None)
}

pub(super) fn render_sandbox_failure_output(
    failure: &SandboxFailureDetails,
    original_output: &str,
) -> String {
    let mut rendered = failure.message().to_string();
    rendered.push_str("\n\n");
    rendered.push_str(failure.escalation().message());

    let trimmed = original_output.trim();
    if !trimmed.is_empty() && trimmed != failure.message() {
        rendered.push_str("\n\nOriginal sandbox output:\n");
        rendered.push_str(trimmed);
    }

    rendered
}

fn approval_denied_result(
    command: &str,
    safety: &BashSafetyAssessment,
    message: &str,
    execution_policy: ExecutionPolicy,
) -> ToolResult {
    text_result_with(
        format!(
            "Bash command blocked: {message}\n\nReason: {}",
            safety.reason
        ),
        Some(json!({
            "command": command,
            "blockedBySafetyPolicy": true,
            "safety": {
                "executionPolicy": execution_policy.as_str(),
                "approvalRequired": true,
                "approved": false,
                "title": safety.title,
                "reason": safety.reason,
            },
        })),
        true,
        None,
    )
}

fn backend_detail(backend: SandboxBackend) -> Value {
    serde_json::to_value(backend).unwrap_or_else(|_| Value::from("bwrap"))
}
