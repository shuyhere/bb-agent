use bb_core::error::BbResult;
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::{Tool, ToolContext, ToolResult};

/// Per-file mutation queue to prevent parallel write conflicts.
pub struct FileQueue {
    locks: Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>,
}

impl FileQueue {
    pub fn new() -> Self {
        Self {
            locks: Mutex::new(HashMap::new()),
        }
    }

    /// Acquire a lock for a specific file path.
    pub async fn lock(&self, path: &PathBuf) -> Arc<Mutex<()>> {
        let mut locks = self.locks.lock().await;
        locks
            .entry(path.clone())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }
}

/// Execute multiple tool calls, potentially in parallel.
/// Write-targeting tools are serialized per file.
pub async fn execute_tool_calls(
    tools: &[Box<dyn Tool>],
    calls: &[(String, String, Value)], // (tool_call_id, tool_name, args)
    ctx: &ToolContext,
    cancel: CancellationToken,
    _file_queue: &FileQueue,
) -> Vec<(String, BbResult<ToolResult>)> {
    let mut handles = Vec::new();

    for (call_id, tool_name, args) in calls {
        let tool = tools.iter().find(|t| t.name() == tool_name);
        let tool_ref = match tool {
            Some(t) => t,
            None => {
                handles.push((
                    call_id.clone(),
                    Err(bb_core::error::BbError::Tool(format!(
                        "Unknown tool: {tool_name}"
                    ))),
                ));
                continue;
            }
        };

        // For simplicity in v1, execute sequentially.
        // Parallel execution with file queue can be added later.
        let result = tool_ref
            .execute(args.clone(), ctx, cancel.clone())
            .await;
        handles.push((call_id.clone(), result));
    }

    handles
}
