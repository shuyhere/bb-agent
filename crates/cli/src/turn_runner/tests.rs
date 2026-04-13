use crate::cache_metrics::RequestCacheMetrics;
use crate::cache_metrics::hydrate_request_metrics_state_from_session_messages;
use crate::cache_metrics::new_shared_request_metrics_state;
use crate::extensions::ExtensionCommandRegistry;
use crate::turn_runner::{TurnConfig, TurnEvent, run_turn, wrap_conn};
use async_trait::async_trait;
use bb_core::error::BbResult;
use bb_core::types::{AgentMessage, ContentBlock, EntryBase, SessionEntry, UserMessage};
use bb_provider::{CompletionRequest, Provider, RequestOptions, StreamEvent};
use bb_session::store;
use bb_tools::{Tool, ToolResult};
use chrono::Utc;
use serde_json::json;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::LazyLock;
use std::sync::Mutex as StdMutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

static REQUEST_METRICS_TEST_LOCK: LazyLock<StdMutex<()>> = LazyLock::new(|| StdMutex::new(()));
static REQUEST_METRICS_SESSION_COUNTER: AtomicUsize = AtomicUsize::new(0);

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

struct CacheAwareProvider {
    call_count: AtomicUsize,
    cache_metrics_source: bb_provider::CacheMetricsSource,
}

#[async_trait]
impl Provider for CacheAwareProvider {
    fn name(&self) -> &str {
        "cache-aware"
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
        let cached = if call_index == 0 { 0 } else { 900 };
        let input = 1000 - cached;
        let _ = tx.send(StreamEvent::Usage(bb_provider::UsageInfo {
            input_tokens: input,
            output_tokens: 10,
            cache_read_tokens: cached,
            cache_write_tokens: 0,
            cache_metrics_source: self.cache_metrics_source.clone(),
        }));
        let _ = tx.send(StreamEvent::TextDelta {
            text: format!("turn-{call_index}"),
        });
        let _ = tx.send(StreamEvent::Done);
        Ok(())
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

fn request_metrics_log_path() -> PathBuf {
    bb_core::config::global_dir().join("request-metrics.jsonl")
}

fn clear_request_metrics_log() {
    let path = request_metrics_log_path();
    let _ = fs::remove_file(path);
}

fn read_request_metrics_log() -> Vec<RequestCacheMetrics> {
    let path = request_metrics_log_path();
    let Ok(content) = fs::read_to_string(path) else {
        return Vec::new();
    };
    content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str::<RequestCacheMetrics>(line).ok())
        .collect()
}

fn read_request_metrics_log_for_session(session_id: &str) -> Vec<RequestCacheMetrics> {
    read_request_metrics_log()
        .into_iter()
        .filter(|row| row.session_id == session_id)
        .collect()
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
        api_key: "dummy".to_string(),
        base_url: "http://dummy.invalid".to_string(),
        headers: std::collections::HashMap::new(),
        compaction_settings: bb_core::types::CompactionSettings::default(),
        tools: vec![Box::new(PanicTool)],
        tool_defs: vec![json!({
            "type": "function",
            "function": {
                "name": "panic-tool",
                "description": "panic test tool",
                "parameters": {"type": "object", "properties": {}}
            }
        })],
        tool_ctx: test_tool_context(),
        thinking: None,
        retry_enabled: false,
        retry_max_retries: 1,
        retry_base_delay_ms: 10,
        retry_max_delay_ms: 10,
        cancel: CancellationToken::new(),
        extensions: ExtensionCommandRegistry::default(),
        request_metrics_state: new_shared_request_metrics_state(),
    };

    let (returned_config, result) = run_turn(config, event_tx, "hi".to_string()).await;
    result.expect("tool panic should be contained without aborting the turn");
    assert_eq!(returned_config.tools.len(), 1);

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
        api_key: "dummy".to_string(),
        base_url: "http://dummy.invalid".to_string(),
        headers: std::collections::HashMap::new(),
        compaction_settings: bb_core::types::CompactionSettings::default(),
        tools: vec![Box::new(EchoTool {
            invocations: invocations.clone(),
        })],
        tool_defs: vec![json!({
            "type": "function",
            "function": {
                "name": "bash",
                "description": "records normalized bash invocations",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "command": {"type": "string"}
                    },
                    "required": ["command"]
                }
            }
        })],
        tool_ctx: test_tool_context(),
        thinking: None,
        retry_enabled: false,
        retry_max_retries: 1,
        retry_base_delay_ms: 10,
        retry_max_delay_ms: 10,
        cancel: CancellationToken::new(),
        extensions: ExtensionCommandRegistry::default(),
        request_metrics_state: new_shared_request_metrics_state(),
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
        api_key: "dummy".to_string(),
        base_url: "http://dummy.invalid".to_string(),
        headers: std::collections::HashMap::new(),
        compaction_settings: bb_core::types::CompactionSettings::default(),
        tools: vec![Box::new(PanicTool)],
        tool_defs: vec![json!({
            "type": "function",
            "function": {
                "name": "panic-tool",
                "description": "panic test tool",
                "parameters": {"type": "object", "properties": {}}
            }
        })],
        tool_ctx: test_tool_context(),
        thinking: None,
        retry_enabled: false,
        retry_max_retries: 1,
        retry_base_delay_ms: 10,
        retry_max_delay_ms: 10,
        cancel,
        extensions: ExtensionCommandRegistry::default(),
        request_metrics_state: new_shared_request_metrics_state(),
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
    let leaf = store::get_entry(&db, &session_id, &leaf_id)
        .expect("get leaf")
        .expect("leaf row");
    let leaf_entry = store::parse_entry(&leaf).expect("parse leaf");

    match leaf_entry {
        SessionEntry::Message {
            message: AgentMessage::ToolResult(result),
            ..
        } => {
            assert!(result.is_error);
            let text = result
                .content
                .iter()
                .filter_map(|block| match block {
                    ContentBlock::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
            assert!(text.contains("tool execution cancelled before start"));
            assert_eq!(
                result
                    .details
                    .as_ref()
                    .and_then(|value| value.get("cancelled"))
                    .and_then(|value| value.as_bool()),
                Some(true)
            );
        }
        other => panic!("expected tool result leaf entry, got {other:?}"),
    }
}

#[tokio::test]
async fn cache_metrics_log_reports_read_hit_rate_for_follow_up_turn() {
    let _guard = REQUEST_METRICS_TEST_LOCK.lock().expect("metrics test lock");
    clear_request_metrics_log();

    let conn = store::open_memory().expect("memory db");
    let session_id = format!(
        "metrics-session-{}",
        REQUEST_METRICS_SESSION_COUNTER.fetch_add(1, Ordering::SeqCst)
    );
    store::create_session_with_id(&conn, &session_id, "/tmp").expect("session");
    let wrapped = wrap_conn(conn);
    let provider = Arc::new(CacheAwareProvider {
        call_count: AtomicUsize::new(0),
        cache_metrics_source: bb_provider::CacheMetricsSource::Official,
    });
    let request_metrics_state = new_shared_request_metrics_state();

    let make_config = || TurnConfig {
        conn: wrapped.clone(),
        session_id: session_id.clone(),
        system_prompt: "system".to_string(),
        model: test_model(128_000),
        provider: provider.clone(),
        api_key: "dummy".to_string(),
        base_url: "http://dummy.invalid".to_string(),
        headers: std::collections::HashMap::new(),
        compaction_settings: bb_core::types::CompactionSettings::default(),
        tools: vec![],
        tool_defs: vec![],
        tool_ctx: test_tool_context(),
        thinking: None,
        retry_enabled: false,
        retry_max_retries: 1,
        retry_base_delay_ms: 10,
        retry_max_delay_ms: 10,
        cancel: CancellationToken::new(),
        extensions: ExtensionCommandRegistry::default(),
        request_metrics_state: request_metrics_state.clone(),
    };

    crate::turn_runner::append_user_message_with_images(&wrapped, &session_id, "first", &[])
        .await
        .expect("append first user");
    let (event_tx1, _event_rx1) = mpsc::unbounded_channel();
    run_turn(make_config(), event_tx1, "first".to_string())
        .await
        .1
        .expect("first turn");

    crate::turn_runner::append_user_message_with_images(&wrapped, &session_id, "second", &[])
        .await
        .expect("append second user");
    let (event_tx2, _event_rx2) = mpsc::unbounded_channel();
    run_turn(make_config(), event_tx2, "second".to_string())
        .await
        .1
        .expect("second turn");

    let metrics = read_request_metrics_log_for_session(&session_id);
    assert!(metrics.len() >= 2, "expected at least two metric rows");

    let last = metrics.last().expect("last metrics row");
    assert_eq!(
        last.cache_metrics_source,
        bb_provider::CacheMetricsSource::Official
    );
    assert_eq!(last.cache_read_tokens, 900);
    assert_eq!(last.input_tokens, 100);
    assert_eq!(last.provider_cache_read_tokens, Some(900));
    assert_eq!(last.estimated_cache_read_tokens, None);
    assert_eq!(last.cache_read_hit_rate_pct, Some(90.0));
    assert_eq!(last.cache_effective_utilization_pct, Some(90.0));
    assert!(last.warm_request);
    assert!(last.previous_request_hash.is_some());
    assert!(last.reused_prefix_bytes_estimate.is_some());
}

#[tokio::test]
async fn cache_metrics_log_uses_estimated_values_for_estimated_provider_metrics() {
    let _guard = REQUEST_METRICS_TEST_LOCK.lock().expect("metrics test lock");
    clear_request_metrics_log();

    let conn = store::open_memory().expect("memory db");
    let session_id = format!(
        "metrics-session-{}",
        REQUEST_METRICS_SESSION_COUNTER.fetch_add(1, Ordering::SeqCst)
    );
    store::create_session_with_id(&conn, &session_id, "/tmp").expect("session");
    let wrapped = wrap_conn(conn);
    let provider = Arc::new(CacheAwareProvider {
        call_count: AtomicUsize::new(0),
        cache_metrics_source: bb_provider::CacheMetricsSource::Estimated,
    });
    let request_metrics_state = new_shared_request_metrics_state();

    let make_config = || TurnConfig {
        conn: wrapped.clone(),
        session_id: session_id.clone(),
        system_prompt: "system".to_string(),
        model: test_model(128_000),
        provider: provider.clone(),
        api_key: "dummy".to_string(),
        base_url: "http://dummy.invalid".to_string(),
        headers: std::collections::HashMap::new(),
        compaction_settings: bb_core::types::CompactionSettings::default(),
        tools: vec![],
        tool_defs: vec![],
        tool_ctx: test_tool_context(),
        thinking: None,
        retry_enabled: false,
        retry_max_retries: 1,
        retry_base_delay_ms: 10,
        retry_max_delay_ms: 10,
        cancel: CancellationToken::new(),
        extensions: ExtensionCommandRegistry::default(),
        request_metrics_state: request_metrics_state.clone(),
    };

    crate::turn_runner::append_user_message_with_images(&wrapped, &session_id, "first", &[])
        .await
        .expect("append first user");
    let (event_tx1, _event_rx1) = mpsc::unbounded_channel();
    run_turn(make_config(), event_tx1, "first".to_string())
        .await
        .1
        .expect("first turn");

    crate::turn_runner::append_user_message_with_images(&wrapped, &session_id, "second", &[])
        .await
        .expect("append second user");
    let (event_tx2, _event_rx2) = mpsc::unbounded_channel();
    run_turn(make_config(), event_tx2, "second".to_string())
        .await
        .1
        .expect("second turn");

    let metrics = read_request_metrics_log_for_session(&session_id);
    assert!(metrics.len() >= 2, "expected at least two metric rows");

    let last = metrics.last().expect("last metrics row");
    assert_eq!(
        last.cache_metrics_source,
        bb_provider::CacheMetricsSource::Estimated
    );
    assert_eq!(last.provider_cache_read_tokens, Some(900));
    assert_eq!(
        last.estimated_cache_read_tokens,
        last.reused_prefix_tokens_estimate
    );
    assert_eq!(
        last.cache_read_tokens,
        last.reused_prefix_tokens_estimate.unwrap_or(0)
    );
    assert!(last.input_tokens < 1000);
    assert!(last.warm_request);
}

#[tokio::test]
async fn cache_metrics_resume_hydration_restores_previous_request_context() {
    let _guard = REQUEST_METRICS_TEST_LOCK.lock().expect("metrics test lock");
    clear_request_metrics_log();

    let conn = store::open_memory().expect("memory db");
    let session_id = format!(
        "metrics-session-{}",
        REQUEST_METRICS_SESSION_COUNTER.fetch_add(1, Ordering::SeqCst)
    );
    store::create_session_with_id(&conn, &session_id, "/tmp").expect("session");
    let wrapped = wrap_conn(conn);
    let provider = Arc::new(CacheAwareProvider {
        call_count: AtomicUsize::new(0),
        cache_metrics_source: bb_provider::CacheMetricsSource::Official,
    });

    crate::turn_runner::append_user_message_with_images(&wrapped, &session_id, "first", &[])
        .await
        .expect("append first user");

    let initial_state = new_shared_request_metrics_state();
    let initial_config = TurnConfig {
        conn: wrapped.clone(),
        session_id: session_id.clone(),
        system_prompt: "system".to_string(),
        model: test_model(128_000),
        provider: provider.clone(),
        api_key: "dummy".to_string(),
        base_url: "http://dummy.invalid".to_string(),
        headers: std::collections::HashMap::new(),
        compaction_settings: bb_core::types::CompactionSettings::default(),
        tools: vec![],
        tool_defs: vec![],
        tool_ctx: test_tool_context(),
        thinking: None,
        retry_enabled: false,
        retry_max_retries: 1,
        retry_base_delay_ms: 10,
        retry_max_delay_ms: 10,
        cancel: CancellationToken::new(),
        extensions: ExtensionCommandRegistry::default(),
        request_metrics_state: initial_state,
    };

    let (event_tx1, _event_rx1) = mpsc::unbounded_channel();
    run_turn(initial_config, event_tx1, "first".to_string())
        .await
        .1
        .expect("first turn");

    let resumed_messages = {
        let conn = wrapped.lock().await;
        bb_session::context::build_context(&conn, &session_id)
            .expect("context")
            .messages
    };

    crate::turn_runner::append_user_message_with_images(&wrapped, &session_id, "second", &[])
        .await
        .expect("append second user");

    let resumed_state = new_shared_request_metrics_state();
    hydrate_request_metrics_state_from_session_messages(
        &resumed_state,
        "system",
        &[],
        &resumed_messages,
        "dummy-model",
        Some(128_000),
        None,
    )
    .await
    .expect("hydrate resumed state");

    let resumed_config = TurnConfig {
        conn: wrapped.clone(),
        session_id: session_id.clone(),
        system_prompt: "system".to_string(),
        model: test_model(128_000),
        provider: provider.clone(),
        api_key: "dummy".to_string(),
        base_url: "http://dummy.invalid".to_string(),
        headers: std::collections::HashMap::new(),
        compaction_settings: bb_core::types::CompactionSettings::default(),
        tools: vec![],
        tool_defs: vec![],
        tool_ctx: test_tool_context(),
        thinking: None,
        retry_enabled: false,
        retry_max_retries: 1,
        retry_base_delay_ms: 10,
        retry_max_delay_ms: 10,
        cancel: CancellationToken::new(),
        extensions: ExtensionCommandRegistry::default(),
        request_metrics_state: resumed_state,
    };

    let (event_tx2, _event_rx2) = mpsc::unbounded_channel();
    run_turn(resumed_config, event_tx2, "second".to_string())
        .await
        .1
        .expect("second turn");

    let metrics = read_request_metrics_log_for_session(&session_id);
    assert!(metrics.len() >= 2, "expected at least two metric rows");

    let last = metrics.last().expect("last metrics row");
    assert!(last.previous_request_hash.is_some());
    assert!(last.first_divergence_byte.is_some());
    assert!(last.reused_prefix_bytes_estimate.is_some());
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
        api_key: "dummy".to_string(),
        base_url: "http://dummy.invalid".to_string(),
        headers: std::collections::HashMap::new(),
        compaction_settings: bb_core::types::CompactionSettings {
            enabled: true,
            reserve_tokens: 0,
            keep_recent_tokens: 1,
        },
        tools: vec![],
        tool_defs: vec![],
        tool_ctx: test_tool_context(),
        thinking: None,
        retry_enabled: false,
        retry_max_retries: 1,
        retry_base_delay_ms: 10,
        retry_max_delay_ms: 10,
        cancel: CancellationToken::new(),
        extensions: ExtensionCommandRegistry::default(),
        request_metrics_state: new_shared_request_metrics_state(),
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
