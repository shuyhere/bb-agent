use async_trait::async_trait;
use bb_core::error::{BbError, BbResult};
use serde_json::{Map, Value, json};
use std::{future, process::Stdio};
use tokio::io::AsyncReadExt;
use tokio::process::{Child, Command};
use tokio_util::sync::CancellationToken;

use crate::artifacts;
use crate::bash_policy::{BashSafetyAssessment, BashSafetyDisposition, classify_bash_command};
use crate::sandbox::{self, PreparedSandboxCommand, SandboxBackend, SandboxFailureDetails};
use crate::support::text_result_with;
use crate::{
    ExecutionPolicy, Tool, ToolApprovalRequest, ToolContext, ToolExecutionMode, ToolResult,
};

const MAX_OUTPUT_LINES: usize = 2000;
const MAX_OUTPUT_BYTES: usize = 50 * 1024;

pub struct BashTool;

#[async_trait]
impl Tool for BashTool {
    fn name(&self) -> &str {
        "bash"
    }

    fn description(&self) -> &str {
        "Execute a bash command in the current working directory. Returns stdout and stderr. \
         Output is truncated to 2000 lines or 50KB (whichever is hit first). \
         Optionally provide a timeout in seconds. \
         In safety mode, read-only commands run inside the sandbox immediately; anything else \
         requires approval in interactive mode and is denied in non-interactive mode."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": { "type": "string", "description": "Bash command to execute" },
                "timeout": { "type": "number", "description": "Timeout in seconds (optional, no default timeout)" }
            },
            "required": ["command"]
        })
    }

    async fn execute(
        &self,
        params: Value,
        ctx: &ToolContext,
        cancel: CancellationToken,
    ) -> BbResult<ToolResult> {
        let command = params
            .get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| BbError::Tool("Missing 'command' parameter".into()))?;

        let timeout_secs = params
            .get("timeout")
            .and_then(|v| v.as_f64())
            .map(std::time::Duration::from_secs_f64);

        let safety = classify_bash_command(command);
        let approval_required = ctx.execution_policy == ExecutionPolicy::Safety
            && matches!(safety.disposition, BashSafetyDisposition::ApprovalRequired);
        let approved = if approval_required {
            match ctx.execution_mode {
                ToolExecutionMode::NonInteractive => {
                    return Ok(approval_denied_result(
                        command,
                        &safety,
                        "Command requires approval in interactive mode and cannot run in non-interactive mode",
                        ctx.execution_policy,
                    ));
                }
                ToolExecutionMode::Interactive => {
                    let Some(request_approval) = ctx.request_approval.as_ref() else {
                        return Ok(approval_denied_result(
                            command,
                            &safety,
                            "Interactive approval UI is unavailable for this command",
                            ctx.execution_policy,
                        ));
                    };
                    let outcome = request_approval(ToolApprovalRequest {
                        tool_name: self.name().to_string(),
                        title: safety.title.clone(),
                        command: command.to_string(),
                        reason: safety.reason.clone(),
                    })
                    .await;
                    if !outcome.approved() {
                        return Ok(approval_denied_result(
                            command,
                            &safety,
                            "Command was denied by the interactive approval flow",
                            ctx.execution_policy,
                        ));
                    }
                    true
                }
            }
        } else {
            false
        };

        let safety_context = BashSafetyContext {
            safety: &safety,
            approval_required,
            approved,
            execution_policy: ctx.execution_policy,
        };

        let SpawnedProcess {
            mut child,
            sandbox_backend,
            #[cfg(unix)]
            process_group_id,
        } = match spawn_bash_process(command, ctx, safety_context) {
            Ok(process) => process,
            Err(result) => return Ok(result),
        };

        let mut stdout = child.stdout.take();
        let mut stderr = child.stderr.take();
        let mut stdout_buf = Vec::new();
        let mut stderr_buf = Vec::new();
        let mut stdout_chunk = [0u8; 4096];
        let mut stderr_chunk = [0u8; 4096];
        let mut status = None;
        let mut cancelled = false;
        let mut timed_out = false;
        let timeout = timeout_secs.map(tokio::time::sleep);
        tokio::pin!(timeout);

        while status.is_none() {
            tokio::select! {
                _ = cancel.cancelled(), if !cancelled => {
                    cancelled = true;
                    kill_running_process(
                        &mut child,
                        #[cfg(unix)]
                        process_group_id,
                    ).await;
                    status = Some(child.wait().await.map_err(|e| BbError::Tool(format!("Failed while waiting for cancelled bash command: {e}")))?);
                }
                _ = async {
                    if let Some(timeout) = timeout.as_mut().as_pin_mut() {
                        timeout.await;
                    } else {
                        future::pending::<()>().await;
                    }
                }, if timeout_secs.is_some() && !timed_out => {
                    timed_out = true;
                    kill_running_process(
                        &mut child,
                        #[cfg(unix)]
                        process_group_id,
                    ).await;
                    status = Some(child.wait().await.map_err(|e| BbError::Tool(format!("Failed while waiting for timed out bash command: {e}")))?);
                }
                result = child.wait() => {
                    status = Some(result.map_err(|e| BbError::Tool(format!("Failed while waiting for bash command: {e}")))?);
                }
                result = async {
                    if let Some(stdout) = stdout.as_mut() {
                        stdout.read(&mut stdout_chunk).await
                    } else {
                        future::pending::<std::io::Result<usize>>().await
                    }
                }, if stdout.is_some() => {
                    let n = result.map_err(|e| BbError::Tool(format!("Failed reading bash stdout: {e}")))?;
                    if n == 0 {
                        stdout = None;
                    } else {
                        let chunk = String::from_utf8_lossy(&stdout_chunk[..n]);
                        if let Some(ref on_output) = ctx.on_output {
                            on_output(&chunk);
                        }
                        stdout_buf.extend_from_slice(&stdout_chunk[..n]);
                    }
                }
                result = async {
                    if let Some(stderr) = stderr.as_mut() {
                        stderr.read(&mut stderr_chunk).await
                    } else {
                        future::pending::<std::io::Result<usize>>().await
                    }
                }, if stderr.is_some() => {
                    let n = result.map_err(|e| BbError::Tool(format!("Failed reading bash stderr: {e}")))?;
                    if n == 0 {
                        stderr = None;
                    } else {
                        stderr_buf.extend_from_slice(&stderr_chunk[..n]);
                    }
                }
            }
        }

        if let Some(stdout) = stdout.as_mut() {
            stdout
                .read_to_end(&mut stdout_buf)
                .await
                .map_err(|e| BbError::Tool(format!("Failed draining bash stdout: {e}")))?;
        }
        if let Some(stderr) = stderr.as_mut() {
            stderr
                .read_to_end(&mut stderr_buf)
                .await
                .map_err(|e| BbError::Tool(format!("Failed draining bash stderr: {e}")))?;
        }

        let exit_code = status.map(|s| s.code().unwrap_or(-1));

        let stdout_str = String::from_utf8_lossy(&stdout_buf);
        let stderr_str = String::from_utf8_lossy(&stderr_buf);

        let mut output = String::new();
        if !stdout_str.is_empty() {
            output.push_str(&stdout_str);
        }
        if !stderr_str.is_empty() {
            if !output.is_empty() {
                output.push('\n');
            }
            output.push_str(&stderr_str);
        }

        let sandbox_failure = sandbox_backend.and_then(|_| {
            if cancelled || timed_out || exit_code.unwrap_or_default() == 0 {
                None
            } else {
                sandbox::classify_sandbox_failure(&stderr_str)
            }
        });

        if let Some(failure) = sandbox_failure.as_ref() {
            output = render_sandbox_failure_output(failure, &output);
        }

        let mut truncated = false;
        let (output, artifact_path) =
            artifacts::maybe_offload(&output, &ctx.artifacts_dir, Some(MAX_OUTPUT_BYTES));
        if artifact_path.is_some() {
            truncated = true;
        } else {
            let lines: Vec<&str> = output.lines().collect();
            if lines.len() > MAX_OUTPUT_LINES {
                let joined = lines[..MAX_OUTPUT_LINES].join("\n");
                let remaining = lines.len() - MAX_OUTPUT_LINES;
                return Ok(text_result_with(
                    format!("{joined}\n\n[{remaining} more lines truncated]"),
                    Some(build_details(BashResultDetails {
                        command,
                        exit_code,
                        cancelled,
                        timed_out,
                        truncated: true,
                        safety: safety_context,
                        sandbox_backend,
                        sandbox_failure: sandbox_failure.as_ref(),
                    })),
                    cancelled || exit_code.map(|c| c != 0).unwrap_or(true),
                    None,
                ));
            }
        }

        Ok(text_result_with(
            output,
            Some(build_details(BashResultDetails {
                command,
                exit_code,
                cancelled,
                timed_out,
                truncated,
                safety: safety_context,
                sandbox_backend,
                sandbox_failure: sandbox_failure.as_ref(),
            })),
            cancelled || exit_code.map(|c| c != 0).unwrap_or(true),
            artifact_path,
        ))
    }
}

struct SpawnedProcess {
    child: Child,
    sandbox_backend: Option<SandboxBackend>,
    #[cfg(unix)]
    process_group_id: Option<u32>,
}

fn spawn_bash_process(
    command: &str,
    ctx: &ToolContext,
    safety: BashSafetyContext<'_>,
) -> Result<SpawnedProcess, ToolResult> {
    match ctx.execution_policy {
        ExecutionPolicy::Yolo => {
            let child = spawn_process(direct_bash_command(command, ctx)).map_err(|error| {
                structured_error_result(
                    format!("Failed to spawn bash: {error}"),
                    BashResultDetails::error(command, safety, None, None),
                )
            })?;
            #[cfg(unix)]
            let process_group_id = child.id();
            Ok(SpawnedProcess {
                child,
                sandbox_backend: None,
                #[cfg(unix)]
                process_group_id,
            })
        }
        ExecutionPolicy::Safety => {
            let PreparedSandboxCommand {
                command: sandboxed,
                backend,
            } = match sandbox::prepare_bash_command(&ctx.cwd, command) {
                Ok(sandboxed) => sandboxed,
                Err(error) => {
                    let details = error.details().clone();
                    return Err(structured_error_result(
                        details.message.clone(),
                        BashResultDetails::error(
                            command,
                            safety,
                            Some(details.backend),
                            Some(&details),
                        ),
                    ));
                }
            };

            let child = spawn_process(configure_process_stdio(sandboxed)).map_err(|error| {
                let details = sandbox::backend_launch_failed_error(
                    backend,
                    format!("Failed to launch Linux sandbox backend: {error}"),
                );
                structured_error_result(
                    details.message.clone(),
                    BashResultDetails::error(command, safety, Some(backend), Some(&details)),
                )
            })?;
            #[cfg(unix)]
            let process_group_id = child.id();

            Ok(SpawnedProcess {
                child,
                sandbox_backend: Some(backend),
                #[cfg(unix)]
                process_group_id,
            })
        }
    }
}

fn direct_bash_command(command: &str, ctx: &ToolContext) -> Command {
    let mut process = Command::new("bash");
    process.arg("-c").arg(command).current_dir(&ctx.cwd);
    configure_process_stdio(process)
}

fn configure_process_stdio(mut process: Command) -> Command {
    process
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    process
}

fn spawn_process(mut process: Command) -> std::io::Result<Child> {
    #[cfg(unix)]
    {
        // Put the shell into its own process group so cancellation/timeouts can
        // terminate the whole command tree instead of only the immediate shell.
        unsafe {
            process.pre_exec(|| {
                if libc::setpgid(0, 0) == 0 {
                    Ok(())
                } else {
                    Err(std::io::Error::last_os_error())
                }
            });
        }
    }

    process.spawn()
}

#[cfg(unix)]
async fn kill_running_process(child: &mut Child, process_group_id: Option<u32>) {
    if let Some(pgid) = process_group_id {
        let _ = send_signal_to_process_group(pgid, libc::SIGTERM);
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    }

    let _ = child.kill().await;

    if let Some(pgid) = process_group_id {
        let _ = send_signal_to_process_group(pgid, libc::SIGKILL);
    }
}

#[cfg(not(unix))]
async fn kill_running_process(child: &mut Child) {
    let _ = child.kill().await;
}

#[cfg(unix)]
fn send_signal_to_process_group(process_group_id: u32, signal: i32) -> std::io::Result<()> {
    let target = -(process_group_id as i32);
    let rc = unsafe { libc::kill(target, signal) };
    if rc == 0 {
        Ok(())
    } else {
        let error = std::io::Error::last_os_error();
        match error.raw_os_error() {
            Some(libc::ESRCH) => Ok(()),
            _ => Err(error),
        }
    }
}

#[derive(Clone, Copy)]
struct BashSafetyContext<'a> {
    safety: &'a BashSafetyAssessment,
    approval_required: bool,
    approved: bool,
    execution_policy: ExecutionPolicy,
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
struct BashResultDetails<'a> {
    command: &'a str,
    exit_code: Option<i32>,
    cancelled: bool,
    timed_out: bool,
    truncated: bool,
    safety: BashSafetyContext<'a>,
    sandbox_backend: Option<SandboxBackend>,
    sandbox_failure: Option<&'a SandboxFailureDetails>,
}

impl<'a> BashResultDetails<'a> {
    fn error(
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

fn build_details(details: BashResultDetails<'_>) -> Value {
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

fn structured_error_result(message: String, details: BashResultDetails<'_>) -> ToolResult {
    text_result_with(message, Some(build_details(details)), true, None)
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

fn render_sandbox_failure_output(failure: &SandboxFailureDetails, original_output: &str) -> String {
    let mut rendered = failure.message.clone();
    rendered.push_str("\n\n");
    rendered.push_str(&failure.escalation.message);

    let trimmed = original_output.trim();
    if !trimmed.is_empty() && trimmed != failure.message {
        rendered.push_str("\n\nOriginal sandbox output:\n");
        rendered.push_str(trimmed);
    }

    rendered
}

fn backend_detail(backend: SandboxBackend) -> Value {
    serde_json::to_value(backend).unwrap_or_else(|_| Value::from("bwrap"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bb_core::types::ContentBlock;
    use std::path::Path;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    fn make_ctx(dir: &Path) -> ToolContext {
        ToolContext {
            cwd: dir.to_path_buf(),
            artifacts_dir: dir.to_path_buf(),
            execution_policy: crate::ExecutionPolicy::Yolo,
            on_output: None,
            web_search: None,
            execution_mode: ToolExecutionMode::Interactive,
            request_approval: Some(Arc::new(|_| {
                Box::pin(async {
                    crate::ToolApprovalOutcome {
                        decision: crate::ToolApprovalDecision::ApprovedOnce,
                    }
                })
            })),
        }
    }

    #[tokio::test]
    async fn bash_collects_stdout_and_stderr_without_deadlock() {
        let dir = tempfile::tempdir().unwrap();
        let tool = BashTool;
        let result = tool
            .execute(
                json!({
                    "command": "for i in $(seq 1 2000); do echo err-$i 1>&2; done; echo done"
                }),
                &make_ctx(dir.path()),
                CancellationToken::new(),
            )
            .await
            .unwrap();

        let text = match &result.content[0] {
            ContentBlock::Text { text } => text,
            other => panic!("unexpected content block: {other:?}"),
        };
        assert!(text.contains("done") || result.artifact_path.is_some());
        assert!(text.contains("err-1"));
        assert!(!text.is_empty());
    }

    #[tokio::test]
    async fn bash_timeout_sets_timed_out_detail() {
        let dir = tempfile::tempdir().unwrap();
        let tool = BashTool;
        let result = tool
            .execute(
                json!({
                    "command": "sleep 1",
                    "timeout": 0.05
                }),
                &make_ctx(dir.path()),
                CancellationToken::new(),
            )
            .await
            .unwrap();

        assert!(result.is_error);
        let details = result.details.unwrap();
        assert_eq!(details["timedOut"], true);
        assert_eq!(details["cancelled"], false);
    }

    #[tokio::test]
    async fn bash_cancellation_sets_cancelled_detail() {
        let dir = tempfile::tempdir().unwrap();
        let tool = BashTool;
        let cancel = CancellationToken::new();
        let canceller = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
            canceller.cancel();
        });

        let result = tool
            .execute(
                json!({
                    "command": "sleep 5"
                }),
                &make_ctx(dir.path()),
                cancel,
            )
            .await
            .unwrap();

        assert!(result.is_error);
        let details = result.details.unwrap();
        assert_eq!(details["cancelled"], true);
        assert_eq!(details["timedOut"], false);
    }

    #[tokio::test]
    async fn bash_denies_approval_needed_commands_in_noninteractive_mode() {
        let dir = tempfile::tempdir().unwrap();
        let tool = BashTool;
        let result = tool
            .execute(
                json!({
                    "command": "cargo check --workspace"
                }),
                &ToolContext {
                    execution_policy: crate::ExecutionPolicy::Safety,
                    execution_mode: ToolExecutionMode::NonInteractive,
                    ..make_ctx(dir.path())
                },
                CancellationToken::new(),
            )
            .await
            .unwrap();

        let text = match &result.content[0] {
            ContentBlock::Text { text } => text,
            other => panic!("unexpected content block: {other:?}"),
        };
        assert!(result.is_error);
        assert!(text.contains("blocked"));
        assert_eq!(
            result
                .details
                .as_ref()
                .and_then(|details| details.get("blockedBySafetyPolicy"))
                .and_then(|value| value.as_bool()),
            Some(true)
        );
    }

    #[tokio::test]
    async fn bash_requests_approval_for_non_read_only_commands() {
        let dir = tempfile::tempdir().unwrap();
        let tool = BashTool;
        let approval_calls = Arc::new(AtomicUsize::new(0));
        let callback_calls = approval_calls.clone();

        let result = tool
            .execute(
                json!({
                    "command": "echo hi > /tmp/out.txt"
                }),
                &ToolContext {
                    execution_policy: crate::ExecutionPolicy::Safety,
                    request_approval: Some(Arc::new(move |request| {
                        let callback_calls = callback_calls.clone();
                        Box::pin(async move {
                            callback_calls.fetch_add(1, Ordering::SeqCst);
                            assert_eq!(request.tool_name, "bash");
                            assert!(request.reason.contains("shell control operators"));
                            crate::ToolApprovalOutcome {
                                decision: crate::ToolApprovalDecision::Denied,
                            }
                        })
                    })),
                    ..make_ctx(dir.path())
                },
                CancellationToken::new(),
            )
            .await
            .unwrap();

        assert_eq!(approval_calls.load(Ordering::SeqCst), 1);
        assert!(result.is_error);
    }

    #[tokio::test]
    async fn safety_mode_requires_sandbox_backend_without_falling_back() {
        let dir = tempfile::tempdir().unwrap();
        let tool = BashTool;
        let original_path = std::env::var_os("PATH");
        unsafe {
            std::env::set_var("PATH", "");
        }

        let result = tool
            .execute(
                json!({
                    "command": "echo should-not-run > sandbox-sentinel"
                }),
                &ToolContext {
                    execution_policy: crate::ExecutionPolicy::Safety,
                    ..make_ctx(dir.path())
                },
                CancellationToken::new(),
            )
            .await
            .unwrap();

        match original_path {
            Some(path) => unsafe { std::env::set_var("PATH", path) },
            None => unsafe { std::env::remove_var("PATH") },
        }

        assert!(result.is_error);
        assert!(!dir.path().join("sandbox-sentinel").exists());
        let details = result.details.unwrap();
        assert_eq!(details["sandbox"]["failure"]["kind"], "backendUnavailable");
        assert_eq!(
            details["sandbox"]["failure"]["escalation"]["action"],
            "rerunWithBroaderPermissions"
        );
    }
}
