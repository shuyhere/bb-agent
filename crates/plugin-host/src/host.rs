use std::path::Path;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tracing::{info, warn};

use crate::protocol::{JsonRpcRequest, JsonRpcResponse};

/// A running plugin host process.
pub struct PluginHost {
    child: Child,
    stdin: tokio::process::ChildStdin,
    stdout_reader: BufReader<tokio::process::ChildStdout>,
}

impl PluginHost {
    /// Spawn a Node.js plugin host process.
    pub async fn spawn(host_script: &Path) -> Result<Self, std::io::Error> {
        let mut child = Command::new("node")
            .arg(host_script)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;

        let stdin = child.stdin.take().expect("stdin");
        let stdout = child.stdout.take().expect("stdout");

        info!("Plugin host spawned (pid: {:?})", child.id());

        Ok(Self {
            child,
            stdin,
            stdout_reader: BufReader::new(stdout),
        })
    }

    /// Send a JSON-RPC notification to the plugin host.
    pub async fn notify(&mut self, method: &str, params: serde_json::Value) -> Result<(), std::io::Error> {
        let req = JsonRpcRequest::notification(method, params);
        let json = serde_json::to_string(&req).unwrap();
        self.stdin.write_all(json.as_bytes()).await?;
        self.stdin.write_all(b"\n").await?;
        self.stdin.flush().await?;
        Ok(())
    }

    /// Read one line of response from the plugin host.
    pub async fn read_response(&mut self) -> Result<Option<JsonRpcResponse>, std::io::Error> {
        let mut line = String::new();
        let bytes = self.stdout_reader.read_line(&mut line).await?;
        if bytes == 0 {
            return Ok(None);
        }
        match serde_json::from_str(&line) {
            Ok(resp) => Ok(Some(resp)),
            Err(e) => {
                warn!("Failed to parse plugin response: {e}");
                Ok(None)
            }
        }
    }

    /// Kill the plugin host process.
    pub async fn kill(&mut self) {
        let _ = self.child.kill().await;
    }
}

impl Drop for PluginHost {
    fn drop(&mut self) {
        // Best-effort kill on drop
        let _ = self.child.start_kill();
    }
}
