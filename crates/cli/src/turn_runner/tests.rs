use super::*;
use async_trait::async_trait;
use bb_core::error::BbResult;
use bb_provider::{CompletionRequest, Provider, RequestOptions, StreamEvent};
use bb_session::store;
use bb_tools::{Tool, ToolResult};
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::mpsc;

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

#[tokio::test]
async fn run_turn_contains_tool_panics_without_aborting_the_turn() {
    let conn = store::open_memory().expect("memory db");
    let session_id = store::create_session(&conn, "/tmp").expect("session");
    let (event_tx, mut event_rx) = mpsc::unbounded_channel();

    let config = TurnConfig {
        conn: wrap_conn(conn),
        session_id,
        system_prompt: "system".to_string(),
        model: bb_provider::registry::Model {
            id: "dummy-model".to_string(),
            name: "dummy-model".to_string(),
            provider: "dummy".to_string(),
            api: bb_provider::registry::ApiType::OpenaiCompletions,
            context_window: 128_000,
            max_tokens: 4_096,
            reasoning: false,
            input: vec![bb_provider::registry::ModelInput::Text],
            base_url: None,
            cost: Default::default(),
        },
        provider: Arc::new(DummyProvider {
            call_count: AtomicUsize::new(0),
        }),
        api_key: "dummy".to_string(),
        base_url: "http://dummy.invalid".to_string(),
        headers: std::collections::HashMap::new(),
        tools: vec![Box::new(PanicTool)],
        tool_defs: vec![json!({
            "type": "function",
            "function": {
                "name": "panic-tool",
                "description": "panic test tool",
                "parameters": {"type": "object", "properties": {}}
            }
        })],
        tool_ctx: bb_tools::ToolContext {
            cwd: "/tmp".into(),
            artifacts_dir: "/tmp".into(),
            execution_policy: bb_tools::ExecutionPolicy::Safety,
            on_output: None,
            web_search: None,
            execution_mode: bb_tools::ToolExecutionMode::Interactive,
            request_approval: None,
        },
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
