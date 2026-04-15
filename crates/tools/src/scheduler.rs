use bb_core::error::{BbError, BbResult};
use futures::future::join_all;
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{Mutex, OwnedMutexGuard, OwnedRwLockReadGuard, OwnedRwLockWriteGuard, RwLock};
use tokio_util::sync::CancellationToken;

use crate::{Tool, ToolContext, ToolResult, ToolScheduling};

/// Per-file mutation queue to prevent parallel write conflicts while still
/// allowing unrelated read-only work and unrelated file mutations to overlap.
pub struct FileQueue {
    locks: Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>,
    mutation_gate: Arc<RwLock<()>>,
}

impl Default for FileQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl FileQueue {
    pub fn new() -> Self {
        Self {
            locks: Mutex::new(HashMap::new()),
            mutation_gate: Arc::new(RwLock::new(())),
        }
    }

    /// Acquire or create the mutex for a specific file path.
    pub async fn lock(&self, path: &Path) -> Arc<Mutex<()>> {
        let mut locks = self.locks.lock().await;
        locks
            .entry(path.to_path_buf())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    pub async fn reserve_scheduling(&self, scheduling: &ToolScheduling) -> FileQueueReservation {
        match scheduling {
            ToolScheduling::ReadOnly => {
                FileQueueReservation::new(FileQueueReservationInner::ReadOnly)
            }
            ToolScheduling::MutatingUnknown => {
                FileQueueReservation::new(FileQueueReservationInner::UnknownMutation {
                    _gate: self.mutation_gate.clone().write_owned().await,
                })
            }
            ToolScheduling::MutatingPaths(paths) => {
                let mut normalized = paths.clone();
                normalized.sort();
                normalized.dedup();

                if normalized.is_empty() {
                    return FileQueueReservation::new(FileQueueReservationInner::UnknownMutation {
                        _gate: self.mutation_gate.clone().write_owned().await,
                    });
                }

                let gate = self.mutation_gate.clone().read_owned().await;
                let mut guards = Vec::with_capacity(normalized.len());
                for path in normalized {
                    let lock = self.lock(&path).await;
                    guards.push(lock.lock_owned().await);
                }
                FileQueueReservation::new(FileQueueReservationInner::KnownMutation {
                    _gate: gate,
                    _guards: guards,
                })
            }
        }
    }
}

pub struct FileQueueReservation(FileQueueReservationInner);

impl FileQueueReservation {
    fn new(inner: FileQueueReservationInner) -> Self {
        Self(inner)
    }

    #[allow(dead_code)]
    fn hold(&self) {
        let _ = &self.0;
    }
}

enum FileQueueReservationInner {
    ReadOnly,
    KnownMutation {
        _gate: OwnedRwLockReadGuard<()>,
        _guards: Vec<OwnedMutexGuard<()>>,
    },
    UnknownMutation {
        _gate: OwnedRwLockWriteGuard<()>,
    },
}

/// Execute a single tool call with mutation-aware scheduling.
pub async fn execute_reserved_tool_call(
    tool: &(dyn Tool + Send + Sync),
    args: Value,
    ctx: &ToolContext,
    cancel: CancellationToken,
    reservation: FileQueueReservation,
) -> BbResult<ToolResult> {
    reservation.hold();
    tool.execute(args, ctx, cancel).await
}

/// Execute a single tool call with mutation-aware scheduling.
pub async fn execute_tool_call(
    tool: &(dyn Tool + Send + Sync),
    args: Value,
    ctx: &ToolContext,
    cancel: CancellationToken,
    file_queue: &FileQueue,
) -> BbResult<ToolResult> {
    let reservation = file_queue
        .reserve_scheduling(&tool.scheduling(&args, ctx))
        .await;
    execute_reserved_tool_call(tool, args, ctx, cancel, reservation).await
}

/// Execute multiple tool calls, allowing read-only and non-conflicting file
/// mutations to overlap while serializing same-file mutation windows.
pub async fn execute_tool_calls(
    tools: &[Box<dyn Tool>],
    calls: &[(String, String, Value)],
    ctx: &ToolContext,
    cancel: CancellationToken,
    file_queue: &FileQueue,
) -> Vec<(String, BbResult<ToolResult>)> {
    let mut pending = Vec::new();
    let mut immediate = Vec::new();

    for (index, (call_id, tool_name, args)) in calls.iter().enumerate() {
        let Some(tool) = tools.iter().find(|tool| tool.name() == tool_name) else {
            immediate.push((
                index,
                call_id.clone(),
                Err(BbError::Tool(format!("Unknown tool: {tool_name}"))),
            ));
            continue;
        };

        let cancel = cancel.clone();
        pending.push(async move {
            let result =
                execute_tool_call(tool.as_ref(), args.clone(), ctx, cancel, file_queue).await;
            (index, call_id.clone(), result)
        });
    }

    let mut results = immediate;
    results.extend(join_all(pending).await);
    results.sort_by_key(|(index, _, _)| *index);
    results
        .into_iter()
        .map(|(_, call_id, result)| (call_id, result))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use bb_core::types::ContentBlock;
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::{Mutex as TokioMutex, Notify};
    use tokio::time::{Duration, sleep, timeout};

    fn test_context() -> ToolContext {
        ToolContext {
            cwd: "/tmp".into(),
            artifacts_dir: "/tmp".into(),
            execution_policy: crate::ExecutionPolicy::Safety,
            on_output: None,
            web_search: None,
            execution_mode: crate::ToolExecutionMode::Interactive,
            request_approval: None,
        }
    }

    fn text_result(text: &str) -> ToolResult {
        ToolResult {
            content: vec![ContentBlock::Text {
                text: text.to_string(),
            }],
            details: None,
            is_error: false,
            artifact_path: None,
        }
    }

    struct CoordinatedReadTool {
        entered: Arc<TokioMutex<usize>>,
        notify: Arc<Notify>,
    }

    #[async_trait]
    impl Tool for CoordinatedReadTool {
        fn name(&self) -> &str {
            "coordinated-read"
        }

        fn description(&self) -> &str {
            "verifies read-only calls can overlap"
        }

        fn parameters_schema(&self) -> Value {
            json!({"type": "object"})
        }

        async fn execute(
            &self,
            _params: Value,
            _ctx: &ToolContext,
            _cancel: CancellationToken,
        ) -> BbResult<ToolResult> {
            let should_wait = {
                let mut entered = self.entered.lock().await;
                *entered += 1;
                let should_wait = *entered < 2;
                if !should_wait {
                    self.notify.notify_waiters();
                }
                should_wait
            };

            if should_wait {
                timeout(Duration::from_millis(200), async {
                    loop {
                        if *self.entered.lock().await >= 2 {
                            break;
                        }
                        self.notify.notified().await;
                    }
                })
                .await
                .map_err(|_| BbError::Tool("read-only tool calls did not overlap".into()))?;
            }

            Ok(text_result("ok"))
        }
    }

    struct MutatingProbeTool {
        active: Arc<AtomicUsize>,
        max_active: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl Tool for MutatingProbeTool {
        fn name(&self) -> &str {
            "mutating-probe"
        }

        fn description(&self) -> &str {
            "tracks concurrent mutation windows"
        }

        fn parameters_schema(&self) -> Value {
            json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"}
                },
                "required": ["path"]
            })
        }

        fn scheduling(&self, params: &Value, ctx: &ToolContext) -> ToolScheduling {
            let path = params
                .get("path")
                .and_then(Value::as_str)
                .map(|path| crate::path::resolve_path(&ctx.cwd, path))
                .unwrap_or_else(|| ctx.cwd.join("unknown"));
            ToolScheduling::single_mutating_path(path)
        }

        async fn execute(
            &self,
            _params: Value,
            _ctx: &ToolContext,
            _cancel: CancellationToken,
        ) -> BbResult<ToolResult> {
            let current = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            let mut observed = self.max_active.load(Ordering::SeqCst);
            while current > observed {
                match self.max_active.compare_exchange(
                    observed,
                    current,
                    Ordering::SeqCst,
                    Ordering::SeqCst,
                ) {
                    Ok(_) => break,
                    Err(next) => observed = next,
                }
            }
            sleep(Duration::from_millis(50)).await;
            self.active.fetch_sub(1, Ordering::SeqCst);
            Ok(text_result("ok"))
        }
    }

    #[tokio::test]
    async fn read_only_calls_can_run_in_parallel() {
        let queue = FileQueue::new();
        let entered = Arc::new(TokioMutex::new(0));
        let notify = Arc::new(Notify::new());
        let tools: Vec<Box<dyn Tool>> = vec![Box::new(CoordinatedReadTool { entered, notify })];
        let calls = vec![
            (
                "call-1".to_string(),
                "coordinated-read".to_string(),
                json!({}),
            ),
            (
                "call-2".to_string(),
                "coordinated-read".to_string(),
                json!({}),
            ),
        ];

        let results = execute_tool_calls(
            &tools,
            &calls,
            &test_context(),
            CancellationToken::new(),
            &queue,
        )
        .await;

        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|(_, result)| result.is_ok()));
    }

    #[tokio::test]
    async fn same_file_mutations_are_serialized() {
        let queue = FileQueue::new();
        let max_active = Arc::new(AtomicUsize::new(0));
        let tools: Vec<Box<dyn Tool>> = vec![Box::new(MutatingProbeTool {
            active: Arc::new(AtomicUsize::new(0)),
            max_active: max_active.clone(),
        })];
        let calls = vec![
            (
                "call-1".to_string(),
                "mutating-probe".to_string(),
                json!({"path": "shared.txt"}),
            ),
            (
                "call-2".to_string(),
                "mutating-probe".to_string(),
                json!({"path": "shared.txt"}),
            ),
        ];

        let results = execute_tool_calls(
            &tools,
            &calls,
            &test_context(),
            CancellationToken::new(),
            &queue,
        )
        .await;

        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|(_, result)| result.is_ok()));
        assert_eq!(max_active.load(Ordering::SeqCst), 1);
    }
}
