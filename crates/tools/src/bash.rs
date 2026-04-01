use async_trait::async_trait;
use bb_core::error::{BbError, BbResult};
use bb_core::types::ContentBlock;
use serde_json::{json, Value};
use std::process::Stdio;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

use crate::artifacts;
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
                "timeout": { "type": "number", "description": "Timeout in seconds (optional)" }
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
            .map(|s| std::time::Duration::from_secs_f64(s));

        let mut child = Command::new("bash")
            .arg("-c")
            .arg(command)
            .current_dir(&ctx.cwd)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| BbError::Tool(format!("Failed to spawn bash: {e}")))?;

        let mut stdout_buf = Vec::new();
        let mut stderr_buf = Vec::new();

        let child_result = async {
            if let Some(ref mut stdout) = child.stdout {
                let _ = stdout.read_to_end(&mut stdout_buf).await;
            }
            if let Some(ref mut stderr) = child.stderr {
                let _ = stderr.read_to_end(&mut stderr_buf).await;
            }
            child.wait().await
        };

        let (status, cancelled) = if let Some(timeout) = timeout_secs {
            tokio::select! {
                result = child_result => (result.ok(), false),
                _ = tokio::time::sleep(timeout) => {
                    let _ = child.kill().await;
                    (None, false)
                },
                _ = cancel.cancelled() => {
                    let _ = child.kill().await;
                    (None, true)
                },
            }
        } else {
            tokio::select! {
                result = child_result => (result.ok(), false),
                _ = cancel.cancelled() => {
                    let _ = child.kill().await;
                    (None, true)
                },
            }
        };

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
                return Ok(ToolResult {
                    content: vec![ContentBlock::Text {
                        text: format!(
                            "{joined}\n\n[{remaining} more lines truncated]"
                        ),
                    }],
                    details: Some(json!({
                        "command": command,
                        "exitCode": exit_code,
                        "cancelled": cancelled,
                        "truncated": true,
                    })),
                    is_error: exit_code.map(|c| c != 0).unwrap_or(true),
                    artifact_path: None,
                });
            }
        }

        Ok(ToolResult {
            content: vec![ContentBlock::Text { text: output }],
            details: Some(json!({
                "command": command,
                "exitCode": exit_code,
                "cancelled": cancelled,
                "truncated": truncated,
            })),
            is_error: cancelled || exit_code.map(|c| c != 0).unwrap_or(true),
            artifact_path,
        })
    }
}
