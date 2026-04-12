use crate::extensions::ExtensionCommandRegistry;
use crate::turn_runner::{TurnConfig, TurnEvent, run_turn, wrap_conn};
use async_trait::async_trait;
use bb_core::error::BbResult;
use bb_core::types::{
    AgentMessage, ContentBlock, EntryBase, SessionEntry, StopReason, UserMessage,
};
use bb_provider::{CompletionRequest, Provider, RequestOptions, StreamEvent};
use bb_session::store;
use bb_tools::{Tool, ToolResult};
use chrono::Utc;
use serde_json::json;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::mpsc;
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
