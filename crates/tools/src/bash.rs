mod output;
mod process;
mod safety;

use async_trait::async_trait;
use bb_core::error::{BbError, BbResult};
use serde_json::{Value, json};
use std::future;
use tokio::io::AsyncReadExt;
use tokio_util::sync::CancellationToken;

use crate::bash_policy::{BashSafetyDisposition, classify_bash_command};
use crate::sandbox;
use crate::support::text_result_with;
use crate::{Tool, ToolContext, ToolResult, ToolScheduling};

#[cfg(test)]
use crate::ToolExecutionMode;

use output::{BashOutputRedactor, redact_bash_output_text, store_bash_output};
use process::{SpawnedProcess, kill_running_process, spawn_bash_process};
use safety::{
    BashResultDetails, BashSafetyContext, build_details, render_sandbox_failure_output,
    request_bash_approval,
};

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

    fn scheduling(&self, params: &Value, _ctx: &ToolContext) -> ToolScheduling {
        let Some(command) = params.get("command").and_then(|value| value.as_str()) else {
            return ToolScheduling::MutatingUnknown;
        };

        match classify_bash_command(command).disposition {
            BashSafetyDisposition::Safe => ToolScheduling::ReadOnly,
            BashSafetyDisposition::ApprovalRequired => ToolScheduling::MutatingUnknown,
        }
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

        let timeout_raw = params.get("timeout").and_then(|v| v.as_f64());
        if let Some(timeout) = timeout_raw
            && (!timeout.is_finite() || timeout <= 0.0)
        {
            return Err(BbError::Tool("bash timeout must be > 0".into()));
        }
        let timeout_secs = timeout_raw.map(std::time::Duration::from_secs_f64);

        let safety = classify_bash_command(command);
        let approved = match request_bash_approval(command, ctx, &safety).await {
            Ok(approved) => approved,
            Err(result) => return Ok(result),
        };

        let safety_context = BashSafetyContext {
            safety: &safety,
            approval_required: ctx.execution_policy == crate::ExecutionPolicy::Safety
                && matches!(
                    safety.disposition,
                    crate::bash_policy::BashSafetyDisposition::ApprovalRequired
                ),
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
        let mut live_redactor = BashOutputRedactor::default();
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
                            let redacted = live_redactor.push(&chunk);
                            if !redacted.is_empty() {
                                on_output(&redacted);
                            }
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
                        let chunk = String::from_utf8_lossy(&stderr_chunk[..n]);
                        if let Some(ref on_output) = ctx.on_output {
                            let redacted = live_redactor.push(&chunk);
                            if !redacted.is_empty() {
                                on_output(&redacted);
                            }
                        }
                        stderr_buf.extend_from_slice(&stderr_chunk[..n]);
                    }
                }
            }
        }

        if let Some(stdout) = stdout.as_mut() {
            // The child may exit before both pipes have been fully read. Drain any remaining bytes
            // through the live redactor so streamed output stays in sync with the final result.
            let drained_from = stdout_buf.len();
            stdout
                .read_to_end(&mut stdout_buf)
                .await
                .map_err(|e| BbError::Tool(format!("Failed draining bash stdout: {e}")))?;
            if let Some(ref on_output) = ctx.on_output {
                let drained = String::from_utf8_lossy(&stdout_buf[drained_from..]);
                let redacted = live_redactor.push(&drained);
                if !redacted.is_empty() {
                    on_output(&redacted);
                }
            }
        }
        if let Some(stderr) = stderr.as_mut() {
            let drained_from = stderr_buf.len();
            stderr
                .read_to_end(&mut stderr_buf)
                .await
                .map_err(|e| BbError::Tool(format!("Failed draining bash stderr: {e}")))?;
            if let Some(ref on_output) = ctx.on_output {
                let drained = String::from_utf8_lossy(&stderr_buf[drained_from..]);
                let redacted = live_redactor.push(&drained);
                if !redacted.is_empty() {
                    on_output(&redacted);
                }
            }
        }

        if let Some(ref on_output) = ctx.on_output {
            let final_redacted = live_redactor.finish();
            if !final_redacted.is_empty() {
                on_output(&final_redacted);
            }
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

        output = redact_bash_output_text(&output);

        let stored_output = store_bash_output(&output, &ctx.artifacts_dir);

        Ok(text_result_with(
            stored_output.output,
            Some(build_details(BashResultDetails {
                command,
                exit_code,
                cancelled,
                timed_out,
                truncated: stored_output.truncated,
                safety: safety_context,
                sandbox_backend,
                sandbox_failure: sandbox_failure.as_ref(),
            })),
            cancelled || exit_code.map(|c| c != 0).unwrap_or(true),
            stored_output.artifact_path,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bb_core::types::ContentBlock;
    use std::path::Path;
    use std::sync::{
        Arc, Mutex,
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
    async fn bash_streams_stdout_and_stderr_chunks_to_on_output() {
        let dir = tempfile::tempdir().unwrap();
        let tool = BashTool;
        let streamed = Arc::new(Mutex::new(String::new()));
        let streamed_clone = streamed.clone();

        let result = tool
            .execute(
                json!({
                    "command": "printf 'out\\n'; printf 'err\\n' 1>&2"
                }),
                &ToolContext {
                    on_output: Some(Box::new(move |chunk| {
                        streamed_clone
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner())
                            .push_str(chunk);
                    })),
                    ..make_ctx(dir.path())
                },
                CancellationToken::new(),
            )
            .await
            .unwrap();

        assert!(!result.is_error);
        let streamed = streamed
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        assert!(streamed.contains("out"));
        assert!(streamed.contains("err"));
    }

    #[tokio::test]
    async fn bash_redacts_live_streamed_secrets_and_final_output() {
        let dir = tempfile::tempdir().unwrap();
        let tool = BashTool;
        let streamed = Arc::new(Mutex::new(String::new()));
        let streamed_clone = streamed.clone();

        let result = tool
            .execute(
                json!({
                    "command": "printf 'Authorization: Bearer '; sleep 0.05; printf 'sk-top-secret\\nOPENAI_API_KEY=sk-inline'"
                }),
                &ToolContext {
                    on_output: Some(Box::new(move |chunk| {
                        streamed_clone
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner())
                            .push_str(chunk);
                    })),
                    ..make_ctx(dir.path())
                },
                CancellationToken::new(),
            )
            .await
            .unwrap();

        let streamed = streamed
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        assert!(streamed.contains("Authorization: Bearer [REDACTED]"));
        assert!(streamed.contains("OPENAI_API_KEY=[REDACTED]"));
        assert!(!streamed.contains("sk-top-secret"));
        assert!(!streamed.contains("sk-inline"));

        let text = match &result.content[0] {
            ContentBlock::Text { text } => text,
            other => panic!("unexpected content block: {other:?}"),
        };
        assert!(text.contains("Authorization: Bearer [REDACTED]"));
        assert!(text.contains("OPENAI_API_KEY=[REDACTED]"));
        assert!(!text.contains("sk-top-secret"));
        assert!(!text.contains("sk-inline"));
    }

    #[tokio::test]
    async fn bash_redacts_offloaded_artifact_output() {
        let dir = tempfile::tempdir().unwrap();
        let tool = BashTool;
        let command =
            "for i in $(seq 1 3000); do printf 'Authorization: Bearer sk-artifact-secret\\n'; done";

        let result = tool
            .execute(
                json!({ "command": command }),
                &make_ctx(dir.path()),
                CancellationToken::new(),
            )
            .await
            .unwrap();

        let artifact_path = result.artifact_path.expect("artifact path");
        let artifact = std::fs::read_to_string(&artifact_path).expect("read artifact");
        assert!(artifact.contains("Authorization: Bearer [REDACTED]"));
        assert!(!artifact.contains("sk-artifact-secret"));
    }

    #[tokio::test]
    async fn bash_truncates_long_output_by_line_count_without_artifact() {
        let dir = tempfile::tempdir().unwrap();
        let tool = BashTool;

        let result = tool
            .execute(
                json!({
                    "command": "for i in $(seq 1 2105); do echo x; done"
                }),
                &make_ctx(dir.path()),
                CancellationToken::new(),
            )
            .await
            .unwrap();

        assert!(result.artifact_path.is_none());
        let text = match &result.content[0] {
            ContentBlock::Text { text } => text,
            other => panic!("unexpected content block: {other:?}"),
        };
        assert!(text.contains("[105 more lines truncated]"));
        assert_eq!(text.lines().filter(|line| *line == "x").count(), 2000);
        assert_eq!(
            result
                .details
                .as_ref()
                .and_then(|details| details.get("truncated")),
            Some(&json!(true))
        );
    }

    #[tokio::test]
    async fn bash_rejects_invalid_timeout() {
        let dir = tempfile::tempdir().unwrap();
        let tool = BashTool;
        let err = tool
            .execute(
                json!({
                    "command": "echo hi",
                    "timeout": 0
                }),
                &make_ctx(dir.path()),
                CancellationToken::new(),
            )
            .await
            .expect_err("zero timeout should be rejected");

        assert!(err.to_string().contains("bash timeout must be > 0"));
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
