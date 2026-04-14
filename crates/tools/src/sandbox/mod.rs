use serde::Serialize;
use serde_json::{Value, json};
use std::path::Path;
use tokio::process::Command;

#[cfg(target_os = "linux")]
mod linux;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum SandboxBackend {
    Bwrap,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum SandboxFailureKind {
    BackendUnavailable,
    LaunchFailed,
    Blocked,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SandboxEscalationRequest {
    action: &'static str,
    target_execution_mode: &'static str,
    message: String,
}

impl SandboxEscalationRequest {
    pub(crate) fn message(&self) -> &str {
        &self.message
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SandboxFailureDetails {
    kind: SandboxFailureKind,
    backend: SandboxBackend,
    message: String,
    escalation: SandboxEscalationRequest,
}

impl SandboxFailureDetails {
    pub(crate) fn backend(&self) -> SandboxBackend {
        self.backend
    }

    pub(crate) fn message(&self) -> &str {
        &self.message
    }

    pub(crate) fn escalation(&self) -> &SandboxEscalationRequest {
        &self.escalation
    }
}

impl SandboxFailureDetails {
    pub(crate) fn to_value(&self) -> Value {
        serde_json::to_value(self).unwrap_or_else(|_| {
            json!({
                "kind": "launchFailed",
                "backend": "bwrap",
                "message": self.message,
            })
        })
    }
}

#[derive(Debug)]
pub(crate) enum SandboxSetupError {
    BackendUnavailable(SandboxFailureDetails),
}

impl SandboxSetupError {
    pub(crate) fn details(&self) -> &SandboxFailureDetails {
        match self {
            Self::BackendUnavailable(details) => details,
        }
    }
}

pub(crate) struct PreparedSandboxCommand {
    command: Command,
    backend: SandboxBackend,
}

impl PreparedSandboxCommand {
    pub(crate) fn into_parts(self) -> (Command, SandboxBackend) {
        (self.command, self.backend)
    }
}

pub(crate) fn prepare_bash_command(
    cwd: &Path,
    command: &str,
) -> Result<PreparedSandboxCommand, SandboxSetupError> {
    #[cfg(target_os = "linux")]
    {
        linux::prepare_bash_command(cwd, command)
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = (cwd, command);
        Err(SandboxSetupError::BackendUnavailable(
            backend_unavailable_error(
                SandboxBackend::Bwrap,
                "Safety mode bash execution is only implemented for Linux right now.".to_string(),
            ),
        ))
    }
}

pub(crate) fn classify_sandbox_failure(stderr: &str) -> Option<SandboxFailureDetails> {
    let stderr = stderr.trim();
    if stderr.is_empty() {
        return None;
    }

    let lower = stderr.to_ascii_lowercase();

    if lower.contains("bwrap:")
        && (lower.contains("creating new namespace failed")
            || lower.contains("no permissions to create new namespace")
            || lower.contains("operation not permitted")
            || lower.contains("permission denied"))
    {
        return Some(backend_launch_failed_error(
            SandboxBackend::Bwrap,
            format!("Linux sandbox backend failed to start: {stderr}"),
        ));
    }

    if lower.contains("permission denied")
        || lower.contains("operation not permitted")
        || lower.contains("read-only file system")
    {
        return Some(blocked_error(
            SandboxBackend::Bwrap,
            format!("Command was blocked by the Linux safety sandbox: {stderr}"),
        ));
    }

    None
}

pub(crate) fn backend_unavailable_error(
    backend: SandboxBackend,
    message: String,
) -> SandboxFailureDetails {
    SandboxFailureDetails {
        kind: SandboxFailureKind::BackendUnavailable,
        backend,
        message,
        escalation: escalation_request(
            "Re-run this command with broader permissions/yolo mode, or install the Linux sandbox backend before retrying in safety mode."
                .to_string(),
        ),
    }
}

pub(crate) fn backend_launch_failed_error(
    backend: SandboxBackend,
    message: String,
) -> SandboxFailureDetails {
    SandboxFailureDetails {
        kind: SandboxFailureKind::LaunchFailed,
        backend,
        message,
        escalation: escalation_request(
            "Re-run this command with broader permissions/yolo mode if it requires capabilities the safety sandbox cannot currently provide."
                .to_string(),
        ),
    }
}

pub(crate) fn blocked_error(backend: SandboxBackend, message: String) -> SandboxFailureDetails {
    SandboxFailureDetails {
        kind: SandboxFailureKind::Blocked,
        backend,
        message,
        escalation: escalation_request(
            "Re-run this command with broader permissions/yolo mode if it needs to write outside the workspace sandbox or access restricted system resources."
                .to_string(),
        ),
    }
}

fn escalation_request(message: String) -> SandboxEscalationRequest {
    SandboxEscalationRequest {
        action: "rerunWithBroaderPermissions",
        target_execution_mode: "yolo",
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_read_only_filesystem_as_blocked() {
        let details = classify_sandbox_failure("touch: /etc/x: Read-only file system").unwrap();
        assert_eq!(details.kind, SandboxFailureKind::Blocked);
        assert_eq!(details.escalation.action, "rerunWithBroaderPermissions");
    }

    #[test]
    fn classifies_bwrap_namespace_failures_as_launch_failures() {
        let details = classify_sandbox_failure(
            "bwrap: Creating new namespace failed: Operation not permitted",
        )
        .unwrap();
        assert_eq!(details.kind, SandboxFailureKind::LaunchFailed);
    }
}
