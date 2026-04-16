use crate::extensions::ExtensionCommandRegistry;
use crate::tool_registry::ToolRegistry;
use crate::turn_runner::{TurnConfig, TurnEvent, run_turn, wrap_conn};
use async_trait::async_trait;
use bb_core::error::BbResult;
use bb_core::types::{
    AgentMessage, CacheMetricsSource, ContentBlock, EntryBase, SessionEntry, StopReason,
    UserMessage,
};
use bb_monitor::RequestMetricsTracker;
use bb_provider::{CompletionRequest, Provider, RequestOptions, StreamEvent, UsageInfo};
use bb_session::store;
use bb_tools::{Tool, ToolResult, ToolScheduling};
use chrono::Utc;
use serde_json::json;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::{Notify, mpsc};
use tokio::time::{Duration, timeout};
use tokio_util::sync::CancellationToken;

struct DummyProvider {
    call_count: AtomicUsize,
}

#[async_trait]
impl Provider for DummyProvider {
    fn name(&self) -> &str {
        "dummy"
    }

    async fn complete(
        &self,
        _request: CompletionRequest,
        _options: RequestOptions,
    ) -> BbResult<Vec<StreamEvent>> {
        Ok(Vec::new())
    }

    async fn stream(
        &self,
        _request: CompletionRequest,
        _options: RequestOptions,
        tx: mpsc::UnboundedSender<StreamEvent>,
    ) -> BbResult<()> {
        let call_index = self.call_count.fetch_add(1, Ordering::SeqCst);
        if call_index == 0 {
            let _ = tx.send(StreamEvent::ToolCallStart {
                id: "tool-1".to_string(),
                name: "panic-tool".to_string(),
            });
            let _ = tx.send(StreamEvent::ToolCallDelta {
                id: "tool-1".to_string(),
                arguments_delta: "{}".to_string(),
            });
        } else {
            let _ = tx.send(StreamEvent::TextDelta {
                text: "done".to_string(),
            });
        }
        let _ = tx.send(StreamEvent::Done);
        Ok(())
    }
}

struct CancelAfterToolCallProvider;

#[async_trait]
impl Provider for CancelAfterToolCallProvider {
    fn name(&self) -> &str {
        "cancel-after-tool-call"
    }

    async fn complete(
        &self,
        _request: CompletionRequest,
        _options: RequestOptions,
    ) -> BbResult<Vec<StreamEvent>> {
        Ok(Vec::new())
    }

    async fn stream(
        &self,
        _request: CompletionRequest,
        options: RequestOptions,
        tx: mpsc::UnboundedSender<StreamEvent>,
    ) -> BbResult<()> {
        let _ = tx.send(StreamEvent::ToolCallStart {
            id: "tool-cancel-1".to_string(),
            name: "panic-tool".to_string(),
        });
        let _ = tx.send(StreamEvent::ToolCallDelta {
            id: "tool-cancel-1".to_string(),
            arguments_delta: "{}".to_string(),
        });
        options.cancel.cancel();
        let _ = tx.send(StreamEvent::Done);
        Ok(())
    }
}

struct AliasProvider {
    call_count: AtomicUsize,
}

#[async_trait]
impl Provider for AliasProvider {
    fn name(&self) -> &str {
        "alias-provider"
    }

    async fn complete(
        &self,
        _request: CompletionRequest,
        _options: RequestOptions,
    ) -> BbResult<Vec<StreamEvent>> {
        Ok(Vec::new())
    }

    async fn stream(
        &self,
        _request: CompletionRequest,
        _options: RequestOptions,
        tx: mpsc::UnboundedSender<StreamEvent>,
    ) -> BbResult<()> {
        let call_index = self.call_count.fetch_add(1, Ordering::SeqCst);
        if call_index == 0 {
            let _ = tx.send(StreamEvent::ToolCallStart {
                id: "tool-alias-1".to_string(),
                name: "functions.Bash".to_string(),
            });
            let _ = tx.send(StreamEvent::ToolCallDelta {
                id: "tool-alias-1".to_string(),
                arguments_delta: r#"{"command":"pwd"}"#.to_string(),
            });
        } else {
            let _ = tx.send(StreamEvent::TextDelta {
                text: "done".to_string(),
            });
        }
        let _ = tx.send(StreamEvent::Done);
        Ok(())
    }
}

struct EchoTool {
    invocations: Arc<std::sync::atomic::AtomicUsize>,
}

#[async_trait]
impl Tool for EchoTool {
    fn name(&self) -> &str {
        "bash"
    }

    fn description(&self) -> &str {
        "records normalized bash invocations"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "command": {"type": "string"}
            },
            "required": ["command"]
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &bb_tools::ToolContext,
        _cancel: CancellationToken,
    ) -> BbResult<ToolResult> {
        self.invocations.fetch_add(1, Ordering::SeqCst);
        let command = params
            .get("command")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        Ok(ToolResult {
            content: vec![ContentBlock::Text {
                text: format!("echoed: {command}"),
            }],
            details: None,
            is_error: false,
            artifact_path: None,
        })
    }
}

struct OverflowProvider;

#[async_trait]
impl Provider for OverflowProvider {
    fn name(&self) -> &str {
        "overflow"
    }

    async fn complete(
        &self,
        _request: CompletionRequest,
        _options: RequestOptions,
    ) -> BbResult<Vec<StreamEvent>> {
        Ok(vec![
            StreamEvent::TextDelta {
                text: "## Goal\nRecover overflow\n\n## Progress\n### Done\n- [x] summarized\n"
                    .to_string(),
            },
            StreamEvent::Done,
        ])
    }

    async fn stream(
        &self,
        _request: CompletionRequest,
        _options: RequestOptions,
        tx: mpsc::UnboundedSender<StreamEvent>,
    ) -> BbResult<()> {
        let _ = tx.send(StreamEvent::Error {
            message: "HTTP 400: context_length_exceeded".to_string(),
        });
        Ok(())
    }
}

struct MetricsProvider;

#[async_trait]
impl Provider for MetricsProvider {
    fn name(&self) -> &str {
        "metrics-provider"
    }

    async fn complete(
        &self,
        _request: CompletionRequest,
        _options: RequestOptions,
    ) -> BbResult<Vec<StreamEvent>> {
        Ok(Vec::new())
    }

    async fn stream(
        &self,
        _request: CompletionRequest,
        _options: RequestOptions,
        tx: mpsc::UnboundedSender<StreamEvent>,
    ) -> BbResult<()> {
        let _ = tx.send(StreamEvent::Usage(UsageInfo {
            input_tokens: 100,
            output_tokens: 20,
            cache_read_tokens: 40,
            cache_write_tokens: 0,
            cache_metrics_source: CacheMetricsSource::Official,
        }));
        let _ = tx.send(StreamEvent::TextDelta {
            text: "done".to_string(),
        });
        let _ = tx.send(StreamEvent::Done);
        Ok(())
    }
}

struct PanicTool;

#[async_trait]
impl Tool for PanicTool {
    fn name(&self) -> &str {
        "panic-tool"
    }

    fn description(&self) -> &str {
        "panic test tool"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {},
        })
    }

    async fn execute(
        &self,
        _params: serde_json::Value,
        _ctx: &bb_tools::ToolContext,
        _cancel: CancellationToken,
    ) -> BbResult<ToolResult> {
        panic!("panic containment test marker");
    }
}

struct MultiToolProvider {
    tool_name: &'static str,
    first_args: &'static str,
    second_args: &'static str,
    call_count: AtomicUsize,
}

#[async_trait]
impl Provider for MultiToolProvider {
    fn name(&self) -> &str {
        "multi-tool"
    }

    async fn complete(
        &self,
        _request: CompletionRequest,
        _options: RequestOptions,
    ) -> BbResult<Vec<StreamEvent>> {
        Ok(Vec::new())
    }

    async fn stream(
        &self,
        _request: CompletionRequest,
        _options: RequestOptions,
        tx: mpsc::UnboundedSender<StreamEvent>,
    ) -> BbResult<()> {
        let call_index = self.call_count.fetch_add(1, Ordering::SeqCst);
        if call_index == 0 {
            let _ = tx.send(StreamEvent::ToolCallStart {
                id: "tool-a".to_string(),
                name: self.tool_name.to_string(),
            });
            let _ = tx.send(StreamEvent::ToolCallDelta {
                id: "tool-a".to_string(),
                arguments_delta: self.first_args.to_string(),
            });
            let _ = tx.send(StreamEvent::ToolCallStart {
                id: "tool-b".to_string(),
                name: self.tool_name.to_string(),
            });
            let _ = tx.send(StreamEvent::ToolCallDelta {
                id: "tool-b".to_string(),
                arguments_delta: self.second_args.to_string(),
            });
        } else {
            let _ = tx.send(StreamEvent::TextDelta {
                text: "done".to_string(),
            });
        }
        let _ = tx.send(StreamEvent::Done);
        Ok(())
    }
}

struct OverlapProbeTool {
    entered: Arc<AtomicUsize>,
    notify: Arc<Notify>,
}

#[async_trait]
impl Tool for OverlapProbeTool {
    fn name(&self) -> &str {
        "overlap-probe"
    }

    fn description(&self) -> &str {
        "verifies read-only tool calls can overlap"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({"type": "object"})
    }

    async fn execute(
        &self,
        _params: serde_json::Value,
        _ctx: &bb_tools::ToolContext,
        _cancel: CancellationToken,
    ) -> BbResult<ToolResult> {
        let entered = self.entered.fetch_add(1, Ordering::SeqCst) + 1;
        if entered < 2 {
            timeout(Duration::from_millis(200), async {
                while self.entered.load(Ordering::SeqCst) < 2 {
                    self.notify.notified().await;
                }
            })
            .await
            .map_err(|_| bb_core::error::BbError::Tool("tool calls did not overlap".into()))?;
        } else {
            self.notify.notify_waiters();
        }

        Ok(ToolResult {
            content: vec![ContentBlock::Text {
                text: "overlap ok".to_string(),
            }],
            details: None,
            is_error: false,
            artifact_path: None,
        })
    }
}

struct SameFileMutationProbeTool {
    active: Arc<AtomicUsize>,
    max_active: Arc<AtomicUsize>,
}

#[async_trait]
impl Tool for SameFileMutationProbeTool {
    fn name(&self) -> &str {
        "same-file-mutation-probe"
    }

    fn description(&self) -> &str {
        "verifies same-file mutation windows are serialized"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"}
            },
            "required": ["path"]
        })
    }

    fn scheduling(
        &self,
        params: &serde_json::Value,
        ctx: &bb_tools::ToolContext,
    ) -> ToolScheduling {
        let path = params
            .get("path")
            .and_then(|value| value.as_str())
            .map(std::path::Path::new)
            .map(|path| {
                if path.is_absolute() {
                    path.to_path_buf()
                } else {
                    ctx.cwd.join(path)
                }
            })
            .unwrap_or_else(|| ctx.cwd.join("unknown"));
        ToolScheduling::single_mutating_path(path)
    }

    async fn execute(
        &self,
        _params: serde_json::Value,
        _ctx: &bb_tools::ToolContext,
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
        tokio::time::sleep(Duration::from_millis(50)).await;
        self.active.fetch_sub(1, Ordering::SeqCst);

        Ok(ToolResult {
            content: vec![ContentBlock::Text {
                text: "mutation ok".to_string(),
            }],
            details: None,
            is_error: false,
            artifact_path: None,
        })
    }
}

fn test_model(context_window: u64) -> bb_provider::registry::Model {
    bb_provider::registry::Model {
        id: "dummy-model".to_string(),
        name: "dummy-model".to_string(),
        provider: "dummy".to_string(),
        api: bb_provider::registry::ApiType::OpenaiCompletions,
        context_window,
        max_tokens: 4_096,
        reasoning: false,
        input: vec![bb_provider::registry::ModelInput::Text],
        base_url: None,
        cost: Default::default(),
    }
}

fn test_request_metrics_tracker() -> Arc<tokio::sync::Mutex<RequestMetricsTracker>> {
    Arc::new(tokio::sync::Mutex::new(RequestMetricsTracker::new()))
}

fn test_tool_context() -> bb_tools::ToolContext {
    bb_tools::ToolContext {
        cwd: "/tmp".into(),
        artifacts_dir: "/tmp".into(),
        execution_policy: bb_tools::ExecutionPolicy::Safety,
        on_output: None,
        web_search: None,
        execution_mode: bb_tools::ToolExecutionMode::Interactive,
        request_approval: None,
    }
}

#[tokio::test]
async fn run_turn_contains_tool_panics_without_aborting_the_turn() {
    let conn = store::open_memory().expect("memory db");
    let session_id = store::create_session(&conn, "/tmp").expect("session");
    let (event_tx, mut event_rx) = mpsc::unbounded_channel();

    let config = TurnConfig {
        conn: wrap_conn(conn),
        session_id,
        system_prompt: "system".to_string(),
        model: test_model(128_000),
        provider: Arc::new(DummyProvider {
            call_count: AtomicUsize::new(0),
        }),
        auth: None,
        api_key: "dummy".to_string(),
        base_url: "http://dummy.invalid".to_string(),
        headers: std::collections::HashMap::new(),
        compaction_settings: bb_core::types::CompactionSettings::default(),
        tool_registry: ToolRegistry::from_tools(vec![Box::new(PanicTool)]),
        tool_ctx: test_tool_context(),
        thinking: None,
        retry_enabled: false,
        retry_max_retries: 1,
        retry_base_delay_ms: 10,
        retry_max_delay_ms: 10,
        cancel: CancellationToken::new(),
        extensions: ExtensionCommandRegistry::default(),
        request_metrics_tracker: test_request_metrics_tracker(),
        request_metrics_log_path: None,
    };

    let (returned_config, result) = run_turn(config, event_tx, "hi".to_string()).await;
    result.expect("tool panic should be contained without aborting the turn");
    assert_eq!(returned_config.tool_registry.len(), 1);

    let mut saw_tool_panic_error = false;
    let mut saw_done = false;
    while let Ok(event) = event_rx.try_recv() {
        match event {
            TurnEvent::ToolResult {
                is_error, content, ..
            } => {
                if is_error {
                    let text = content
                        .iter()
                        .filter_map(|block| match block {
                            ContentBlock::Text { text } => Some(text.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    if text.contains("panic containment test marker") {
                        saw_tool_panic_error = true;
                    }
                }
            }
            TurnEvent::Done { text } => {
                saw_done = true;
                assert_eq!(text, "done");
            }
            _ => {}
        }
    }
    assert!(
        saw_tool_panic_error,
        "should convert tool panic into tool error output"
    );
    assert!(
        saw_done,
        "turn should still complete after contained tool panic"
    );
}

#[tokio::test]
async fn run_turn_normalizes_builtin_tool_aliases_before_lookup() {
    let conn = store::open_memory().expect("memory db");
    let session_id = store::create_session(&conn, "/tmp").expect("session");
    let (event_tx, mut event_rx) = mpsc::unbounded_channel();
    let invocations = Arc::new(AtomicUsize::new(0));

    let config = TurnConfig {
        conn: wrap_conn(conn),
        session_id,
        system_prompt: "system".to_string(),
        model: test_model(128_000),
        provider: Arc::new(AliasProvider {
            call_count: AtomicUsize::new(0),
        }),
        auth: None,
        api_key: "dummy".to_string(),
        base_url: "http://dummy.invalid".to_string(),
        headers: std::collections::HashMap::new(),
        compaction_settings: bb_core::types::CompactionSettings::default(),
        tool_registry: ToolRegistry::from_tools(vec![Box::new(EchoTool {
            invocations: invocations.clone(),
        })]),
        tool_ctx: test_tool_context(),
        thinking: None,
        retry_enabled: false,
        retry_max_retries: 1,
        retry_base_delay_ms: 10,
        retry_max_delay_ms: 10,
        cancel: CancellationToken::new(),
        extensions: ExtensionCommandRegistry::default(),
        request_metrics_tracker: test_request_metrics_tracker(),
        request_metrics_log_path: None,
    };

    let (_returned_config, result) = run_turn(config, event_tx, "hi".to_string()).await;
    result.expect("aliased builtin tool should resolve successfully");
    assert_eq!(invocations.load(Ordering::SeqCst), 1);

    let mut saw_successful_tool_result = false;
    let mut saw_done = false;
    while let Ok(event) = event_rx.try_recv() {
        match event {
            TurnEvent::ToolResult {
                is_error, content, ..
            } => {
                let text = content
                    .iter()
                    .filter_map(|block| match block {
                        ContentBlock::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                if !is_error && text.contains("echoed: pwd") {
                    saw_successful_tool_result = true;
                }
            }
            TurnEvent::Done { text } => {
                saw_done = true;
                assert_eq!(text, "done");
            }
            _ => {}
        }
    }

    assert!(
        saw_successful_tool_result,
        "normalized alias should execute the builtin tool"
    );
    assert!(saw_done, "turn should complete after the aliased tool call");
}

#[tokio::test]
async fn cancelled_turn_with_tool_calls_persists_cancelled_tool_results() {
    let conn = store::open_memory().expect("memory db");
    let session_id = store::create_session(&conn, "/tmp").expect("session");
    let wrapped = wrap_conn(conn);
    let (event_tx, mut event_rx) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();

    let config = TurnConfig {
        conn: wrapped.clone(),
        session_id: session_id.clone(),
        system_prompt: "system".to_string(),
        model: test_model(128_000),
        provider: Arc::new(CancelAfterToolCallProvider),
        auth: None,
        api_key: "dummy".to_string(),
        base_url: "http://dummy.invalid".to_string(),
        headers: std::collections::HashMap::new(),
        compaction_settings: bb_core::types::CompactionSettings::default(),
        tool_registry: ToolRegistry::from_tools(vec![Box::new(PanicTool)]),
        tool_ctx: test_tool_context(),
        thinking: None,
        retry_enabled: false,
        retry_max_retries: 1,
        retry_base_delay_ms: 10,
        retry_max_delay_ms: 10,
        cancel,
        extensions: ExtensionCommandRegistry::default(),
        request_metrics_tracker: test_request_metrics_tracker(),
        request_metrics_log_path: None,
    };

    let (_returned_config, result) = run_turn(config, event_tx, "hi".to_string()).await;
    result.expect("cancelled turn should remain transcript-safe");

    let mut saw_cancelled_tool_result = false;
    while let Ok(event) = event_rx.try_recv() {
        if let TurnEvent::ToolResult {
            is_error,
            details,
            content,
            ..
        } = event
        {
            let text = content
                .iter()
                .filter_map(|block| match block {
                    ContentBlock::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
            if is_error
                && text.contains("tool execution cancelled before start")
                && details
                    .as_ref()
                    .and_then(|value| value.get("cancelled"))
                    .and_then(|value| value.as_bool())
                    == Some(true)
            {
                saw_cancelled_tool_result = true;
            }
        }
    }
    assert!(
        saw_cancelled_tool_result,
        "should emit a cancelled tool result event"
    );

    let db = wrapped.lock().await;
    let session = store::get_session(&db, &session_id)
        .expect("get session")
        .expect("session exists");
    let leaf_id = session.leaf_id.expect("leaf id");
    let path = bb_session::tree::walk_to_root(&db, &session_id, &leaf_id).expect("path to root");
    let messages = path
        .into_iter()
        .filter_map(|entry| store::parse_entry(&entry).ok())
        .filter_map(|entry| match entry {
            SessionEntry::Message { message, .. } => Some(message),
            _ => None,
        })
        .collect::<Vec<_>>();

    assert!(matches!(
        messages.iter().find(|message| matches!(message, AgentMessage::Assistant(_))),
        Some(AgentMessage::Assistant(assistant)) if assistant.stop_reason == StopReason::ToolUse
    ));
    assert!(matches!(
        messages.iter().find(|message| matches!(message, AgentMessage::ToolResult(_))),
        Some(AgentMessage::ToolResult(tool_result))
            if tool_result.tool_call_id == "tool-cancel-1"
                && tool_result.is_error
                && tool_result.details.as_ref().and_then(|value| value.get("cancelled")).and_then(|value| value.as_bool()) == Some(true)
    ));
}

#[tokio::test]
async fn read_only_tool_calls_can_overlap_in_real_turn_execution() {
    let conn = store::open_memory().expect("memory db");
    let session_id = store::create_session(&conn, "/tmp").expect("session");
    let (event_tx, mut event_rx) = mpsc::unbounded_channel();
    let entered = Arc::new(AtomicUsize::new(0));
    let notify = Arc::new(Notify::new());

    let config = TurnConfig {
        conn: wrap_conn(conn),
        session_id,
        system_prompt: "system".to_string(),
        model: test_model(128_000),
        provider: Arc::new(MultiToolProvider {
            tool_name: "overlap-probe",
            first_args: "{}",
            second_args: "{}",
            call_count: AtomicUsize::new(0),
        }),
        auth: None,
        api_key: "dummy".to_string(),
        base_url: "http://dummy.invalid".to_string(),
        headers: std::collections::HashMap::new(),
        compaction_settings: bb_core::types::CompactionSettings::default(),
        tool_registry: ToolRegistry::from_tools(vec![Box::new(OverlapProbeTool {
            entered,
            notify,
        })]),
        tool_ctx: test_tool_context(),
        thinking: None,
        retry_enabled: false,
        retry_max_retries: 1,
        retry_base_delay_ms: 10,
        retry_max_delay_ms: 10,
        cancel: CancellationToken::new(),
        extensions: ExtensionCommandRegistry::default(),
        request_metrics_tracker: test_request_metrics_tracker(),
        request_metrics_log_path: None,
    };

    let (_returned_config, result) = run_turn(config, event_tx, "hi".to_string()).await;
    result.expect("read-only tool calls should be allowed to overlap");

    let mut saw_error = false;
    let mut saw_done = false;
    while let Ok(event) = event_rx.try_recv() {
        match event {
            TurnEvent::ToolResult { is_error, .. } => saw_error |= is_error,
            TurnEvent::Done { text } => {
                saw_done = true;
                assert_eq!(text, "done");
            }
            _ => {}
        }
    }

    assert!(
        !saw_error,
        "parallel read-only execution should not time out"
    );
    assert!(
        saw_done,
        "turn should complete after overlapping tool calls"
    );
}

#[tokio::test]
async fn same_file_mutations_stay_serialized_in_real_turn_execution() {
    let conn = store::open_memory().expect("memory db");
    let session_id = store::create_session(&conn, "/tmp").expect("session");
    let (event_tx, mut event_rx) = mpsc::unbounded_channel();
    let active = Arc::new(AtomicUsize::new(0));
    let max_active = Arc::new(AtomicUsize::new(0));

    let config = TurnConfig {
        conn: wrap_conn(conn),
        session_id,
        system_prompt: "system".to_string(),
        model: test_model(128_000),
        provider: Arc::new(MultiToolProvider {
            tool_name: "same-file-mutation-probe",
            first_args: r#"{"path":"shared.txt"}"#,
            second_args: r#"{"path":"shared.txt"}"#,
            call_count: AtomicUsize::new(0),
        }),
        auth: None,
        api_key: "dummy".to_string(),
        base_url: "http://dummy.invalid".to_string(),
        headers: std::collections::HashMap::new(),
        compaction_settings: bb_core::types::CompactionSettings::default(),
        tool_registry: ToolRegistry::from_tools(vec![Box::new(SameFileMutationProbeTool {
            active,
            max_active: max_active.clone(),
        })]),
        tool_ctx: test_tool_context(),
        thinking: None,
        retry_enabled: false,
        retry_max_retries: 1,
        retry_base_delay_ms: 10,
        retry_max_delay_ms: 10,
        cancel: CancellationToken::new(),
        extensions: ExtensionCommandRegistry::default(),
        request_metrics_tracker: test_request_metrics_tracker(),
        request_metrics_log_path: None,
    };

    let (_returned_config, result) = run_turn(config, event_tx, "hi".to_string()).await;
    result.expect("same-file mutations should serialize safely");
    assert_eq!(max_active.load(Ordering::SeqCst), 1);

    let mut saw_done = false;
    while let Ok(event) = event_rx.try_recv() {
        if let TurnEvent::Done { text } = event {
            saw_done = true;
            assert_eq!(text, "done");
        }
    }
    assert!(saw_done, "turn should complete after serialized mutations");
}

#[tokio::test]
async fn overflow_recovery_compacts_only_active_path_context() {
    let dir = tempfile::tempdir().expect("temp dir");
    let db_path = dir.path().join("session.db");
    let conn = store::open_db(&db_path).expect("db");
    let session_id = store::create_session(&conn, "/tmp").expect("session");

    let root = SessionEntry::Message {
        base: EntryBase {
            id: bb_core::types::EntryId("root0001".into()),
            parent_id: None,
            timestamp: Utc::now(),
        },
        message: AgentMessage::User(UserMessage {
            content: vec![ContentBlock::Text {
                text: "old ".repeat(400_000),
            }],
            timestamp: Utc::now().timestamp_millis(),
        }),
    };
    store::append_entry(&conn, &session_id, &root).expect("append root");

    let historical = SessionEntry::Message {
        base: EntryBase {
            id: bb_core::types::EntryId("hist0002".into()),
            parent_id: Some(bb_core::types::EntryId("root0001".into())),
            timestamp: Utc::now(),
        },
        message: AgentMessage::User(UserMessage {
            content: vec![ContentBlock::Text {
                text: "historical ".repeat(400_000),
            }],
            timestamp: Utc::now().timestamp_millis(),
        }),
    };
    store::append_entry(&conn, &session_id, &historical).expect("append historical");

    store::set_leaf(&conn, &session_id, Some("root0001")).expect("set leaf to root branch");

    let active = SessionEntry::Message {
        base: EntryBase {
            id: bb_core::types::EntryId("actv0003".into()),
            parent_id: Some(bb_core::types::EntryId("root0001".into())),
            timestamp: Utc::now(),
        },
        message: AgentMessage::User(UserMessage {
            content: vec![ContentBlock::Text {
                text: "active branch prompt".to_string(),
            }],
            timestamp: Utc::now().timestamp_millis(),
        }),
    };
    store::append_entry(&conn, &session_id, &active).expect("append active");

    let (event_tx, mut event_rx) = mpsc::unbounded_channel();
    let config = TurnConfig {
        conn: wrap_conn(conn),
        session_id: session_id.clone(),
        system_prompt: "system".to_string(),
        model: test_model(10_000_000),
        provider: Arc::new(OverflowProvider),
        auth: None,
        api_key: "dummy".to_string(),
        base_url: "http://dummy.invalid".to_string(),
        headers: std::collections::HashMap::new(),
        compaction_settings: bb_core::types::CompactionSettings {
            enabled: true,
            reserve_tokens: 0,
            keep_recent_tokens: 1,
        },
        tool_registry: ToolRegistry::default(),
        tool_ctx: test_tool_context(),
        thinking: None,
        retry_enabled: false,
        retry_max_retries: 1,
        retry_base_delay_ms: 10,
        retry_max_delay_ms: 10,
        cancel: CancellationToken::new(),
        extensions: ExtensionCommandRegistry::default(),
        request_metrics_tracker: test_request_metrics_tracker(),
        request_metrics_log_path: None,
    };

    let (_returned_config, result) =
        run_turn(config, event_tx, "trigger overflow".to_string()).await;
    result.expect("overflow recovery should complete without fatal error");

    let statuses = std::iter::from_fn(|| event_rx.try_recv().ok())
        .filter_map(|event| match event {
            TurnEvent::Status(message) => Some(message),
            _ => None,
        })
        .collect::<Vec<_>>();

    let auto_status = statuses
        .iter()
        .find(|message| message.starts_with("Auto-compacted session:"))
        .expect("auto-compaction status");
    assert!(
        !auto_status.contains("200000")
            && !auto_status.contains("400000")
            && !auto_status.contains("800000"),
        "auto-compaction should not report total historical session size: {auto_status}"
    );

    let append_conn = store::open_db(&db_path).expect("reopen db");
    let path = bb_session::tree::active_path(&append_conn, &session_id).expect("active path");
    assert_eq!(
        path.len(),
        3,
        "root + active + compaction on active branch only"
    );
    assert_eq!(path[0].entry_id, "root0001");
    assert_eq!(path[1].entry_id, "actv0003");
    assert_eq!(path[2].entry_type, "compaction");
}

#[tokio::test]
async fn run_turn_writes_request_metrics_log_when_path_is_configured() {
    let conn = store::open_memory().expect("memory db");
    let session_id = store::create_session(&conn, "/tmp").expect("session");
    let (event_tx, _event_rx) = mpsc::unbounded_channel();
    let temp = tempfile::tempdir().expect("tempdir");
    let log_path = temp.path().join("request-metrics.jsonl");

    let config = TurnConfig {
        conn: wrap_conn(conn),
        session_id,
        system_prompt: "system".to_string(),
        model: test_model(128_000),
        provider: Arc::new(MetricsProvider),
        auth: None,
        api_key: "dummy".to_string(),
        base_url: "http://dummy.invalid".to_string(),
        headers: std::collections::HashMap::new(),
        compaction_settings: bb_core::types::CompactionSettings::default(),
        tool_registry: ToolRegistry::default(),
        tool_ctx: test_tool_context(),
        thinking: None,
        retry_enabled: false,
        retry_max_retries: 1,
        retry_base_delay_ms: 10,
        retry_max_delay_ms: 10,
        cancel: CancellationToken::new(),
        extensions: ExtensionCommandRegistry::default(),
        request_metrics_tracker: test_request_metrics_tracker(),
        request_metrics_log_path: Some(log_path.clone()),
    };

    let (_returned_config, result) = run_turn(config, event_tx, "hi".to_string()).await;
    result.expect("turn should succeed and write request metrics");

    let written = std::fs::read_to_string(&log_path).expect("request metrics log");
    assert!(written.contains("\"provider\":\"metrics-provider\""));
    assert!(written.contains("\"model\":\"dummy-model\""));
    assert!(written.contains("\"cache_metrics_source\":\"official\""));
    assert!(written.contains("\"cache_read_tokens\":40"));
}
