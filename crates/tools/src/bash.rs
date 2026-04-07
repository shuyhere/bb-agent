use async_trait::async_trait;
use bb_core::error::{BbError, BbResult};
use serde_json::{Value, json};
use std::{future, process::Stdio};
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

use crate::artifacts;
use crate::support::text_result_with;
use crate::{Tool, ToolContext, ToolResult};

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
         Optionally provide a timeout in seconds."
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

        let mut child = Command::new("bash")
            .arg("-c")
            .arg(command)
            .current_dir(&ctx.cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| BbError::Tool(format!("Failed to spawn bash: {e}")))?;

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
                    let _ = child.kill().await;
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
                    let _ = child.kill().await;
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

        // Truncate
        let mut truncated = false;
        let (output, artifact_path) =
            artifacts::maybe_offload(&output, &ctx.artifacts_dir, Some(MAX_OUTPUT_BYTES));
        if artifact_path.is_some() {
            truncated = true;
        } else {
            // Line-based truncation
            let lines: Vec<&str> = output.lines().collect();
            if lines.len() > MAX_OUTPUT_LINES {
                let joined = lines[..MAX_OUTPUT_LINES].join("\n");
                let remaining = lines.len() - MAX_OUTPUT_LINES;
                return Ok(text_result_with(
                    format!("{joined}\n\n[{remaining} more lines truncated]"),
                    Some(json!({
                        "command": command,
                        "exitCode": exit_code,
                        "cancelled": cancelled,
                        "timedOut": timed_out,
                        "truncated": true,
                    })),
                    exit_code.map(|c| c != 0).unwrap_or(true),
                    None,
                ));
            }
        }

        Ok(text_result_with(
            output,
            Some(json!({
                "command": command,
                "exitCode": exit_code,
                "cancelled": cancelled,
                "timedOut": timed_out,
                "truncated": truncated,
            })),
            cancelled || exit_code.map(|c| c != 0).unwrap_or(true),
            artifact_path,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bb_core::types::ContentBlock;
    use std::path::Path;

    fn make_ctx(dir: &Path) -> ToolContext {
        ToolContext {
            cwd: dir.to_path_buf(),
            artifacts_dir: dir.to_path_buf(),
            on_output: None,
            web_search: None,
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
}
