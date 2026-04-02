//! Shared streaming turn loop used by both print mode and interactive mode.
//!
//! Extracts the duplicated logic for:
//! - Building CompletionRequest
//! - Calling provider.stream()
//! - Collecting stream events
//! - Building assistant messages and appending entries to session DB
//! - Executing tool calls
//! - Looping for multi-turn tool use

use anyhow::Result;
use bb_core::agent_loop::compat::is_context_overflow;
use bb_core::agent_session::messages_to_provider;
use bb_core::types::*;
use bb_provider::registry::Model;
use bb_provider::streaming::CollectedResponse;
use bb_provider::{CompletionRequest, Provider, RequestOptions, StreamEvent};
use bb_session::{context, store};
use bb_tools::{Tool, ToolContext};
use chrono::Utc;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// Configuration for a streaming turn loop.
pub struct TurnConfig {
    pub conn: Arc<Mutex<rusqlite::Connection>>,
    pub session_id: String,
    pub system_prompt: String,
    pub model: Model,
    pub provider: Arc<dyn Provider>,
    pub api_key: String,
    pub base_url: String,
    pub tools: Vec<Box<dyn Tool>>,
    pub tool_defs: Vec<serde_json::Value>,
    pub tool_ctx: ToolContext,
    pub thinking: Option<String>,
    pub cancel: CancellationToken,
}

/// Events emitted during a streaming turn for the UI to consume.
#[derive(Clone, Debug)]
pub enum TurnEvent {
    TurnStart { turn_index: u32 },
    TextDelta(String),
    ThinkingDelta(String),
    ToolCallStart { id: String, name: String },
    ToolCallDelta { id: String, args: String },
    ToolExecuting { id: String, name: String },
    ToolResult {
        id: String,
        name: String,
        content: Vec<ContentBlock>,
        details: Option<serde_json::Value>,
        artifact_path: Option<String>,
        is_error: bool,
    },
    TurnEnd { turn_index: u32 },
    ContextOverflow { message: String },
    Done { text: String },
    Error(String),
}

/// Run the full multi-turn streaming loop: stream from provider, execute tools,
/// loop until the assistant produces a final text response (no more tool calls).
///
/// Events are sent on `event_tx` so the caller can render them however it likes.
/// The function respects `config.cancel` for abort.
/// Run the turn loop. Takes ownership of config and returns it when done
/// (so the caller can recover owned resources like tools and connection).
/// Run the turn loop. Takes ownership of config and returns it when done
/// (so the caller can recover owned resources like tools and connection).
pub async fn run_turn(
    config: TurnConfig,
    event_tx: mpsc::UnboundedSender<TurnEvent>,
) -> (TurnConfig, Result<()>) {
    let result = run_turn_inner(&config, &event_tx).await;
    (config, result)
}

pub(crate) async fn run_turn_inner(
    config: &TurnConfig,
    event_tx: &mpsc::UnboundedSender<TurnEvent>,
) -> Result<()> {
    let mut turn_index: u32 = 0;

    loop {
        let _ = event_tx.send(TurnEvent::TurnStart { turn_index });

        if config.cancel.is_cancelled() {
            let _ = event_tx.send(TurnEvent::Done {
                text: String::new(),
            });
            break;
        }

        // Build context from session
        let conn = config.conn.lock().await;
        let ctx = context::build_context(&conn, &config.session_id)?;
        drop(conn);
        let provider_messages = messages_to_provider(&ctx.messages);

        let request = CompletionRequest {
            system_prompt: config.system_prompt.clone(),
            messages: provider_messages,
            tools: config.tool_defs.clone(),
            model: config.model.id.clone(),
            max_tokens: Some(config.model.max_tokens as u32),
            stream: true,
            thinking: config.thinking.clone(),
        };

        let options = RequestOptions {
            api_key: config.api_key.clone(),
            base_url: config.base_url.clone(),
            headers: std::collections::HashMap::new(),
            cancel: config.cancel.clone(),
        };

        // Spawn provider streaming in a background task
        let (stream_tx, mut stream_rx) = mpsc::unbounded_channel();
        let provider = config.provider.clone();
        let stream_cancel = config.cancel.clone();
        let stream_handle = tokio::spawn(async move {
            let result = provider.stream(request, options, stream_tx).await;
            if let Err(e) = result {
                if !stream_cancel.is_cancelled() {
                    return Err(e);
                }
            }
            Ok(())
        });

        // Collect stream events, forwarding deltas to the caller
        let mut all_events = Vec::new();
        let mut context_overflow_error: Option<String> = None;

        while let Some(event) = stream_rx.recv().await {
            match &event {
                StreamEvent::TextDelta { text } => {
                    let _ = event_tx.send(TurnEvent::TextDelta(text.clone()));
                }
                StreamEvent::ThinkingDelta { text } => {
                    let _ = event_tx.send(TurnEvent::ThinkingDelta(text.clone()));
                }
                StreamEvent::ToolCallStart { id, name } => {
                    let _ = event_tx.send(TurnEvent::ToolCallStart {
                        id: id.clone(),
                        name: name.clone(),
                    });
                }
                StreamEvent::ToolCallDelta {
                    id,
                    arguments_delta,
                } => {
                    let _ = event_tx.send(TurnEvent::ToolCallDelta {
                        id: id.clone(),
                        args: arguments_delta.clone(),
                    });
                }
                StreamEvent::Error { message } => {
                    if is_context_overflow(message) {
                        context_overflow_error = Some(message.clone());
                    }
                    let _ = event_tx.send(TurnEvent::Error(message.clone()));
                }
                _ => {}
            }
            all_events.push(event);

            if config.cancel.is_cancelled() {
                break;
            }
        }

        // Wait for stream task to finish
        let _ = stream_handle.await;

        if config.cancel.is_cancelled() {
            let _ = event_tx.send(TurnEvent::Done {
                text: String::new(),
            });
            break;
        }

        // Signal context overflow so the caller can handle compaction + retry
        if let Some(ref overflow_msg) = context_overflow_error {
            let _ = event_tx.send(TurnEvent::ContextOverflow {
                message: overflow_msg.clone(),
            });
            // The caller is responsible for compaction; we stop here.
            // If the caller wants to retry, it calls run_turn again.
            break;
        }

        let collected = CollectedResponse::from_events(&all_events);

        // Append assistant message to session DB
        {
            let conn = config.conn.lock().await;
            append_assistant_message(&conn, &config.session_id, &config.model, &collected)?;
        }

        let _ = event_tx.send(TurnEvent::TurnEnd { turn_index });

        // If no tool calls, we're done
        if collected.tool_calls.is_empty() {
            let _ = event_tx.send(TurnEvent::Done {
                text: collected.text,
            });
            break;
        }

        // Execute tool calls
        if config.cancel.is_cancelled() {
            let _ = event_tx.send(TurnEvent::Done {
                text: collected.text,
            });
            break;
        }

        execute_tool_calls(
            &config.conn,
            &config.session_id,
            &collected,
            &config.tools,
            &config.tool_ctx,
            &config.cancel,
            event_tx,
        )
        .await?;

        turn_index += 1;
    }

    Ok(())
}

// =============================================================================
// Shared helpers (previously duplicated in run.rs and runtime.rs)
// =============================================================================

pub async fn append_user_message(
    conn: &Arc<Mutex<rusqlite::Connection>>,
    session_id: &str,
    prompt: &str,
) -> Result<()> {
    let conn = conn.lock().await;
    let user_entry = SessionEntry::Message {
        base: EntryBase {
            id: EntryId::generate(),
            parent_id: get_leaf_raw(&conn, session_id),
            timestamp: Utc::now(),
        },
        message: AgentMessage::User(UserMessage {
            content: vec![ContentBlock::Text {
                text: prompt.to_string(),
            }],
            timestamp: Utc::now().timestamp_millis(),
        }),
    };
    store::append_entry(&conn, session_id, &user_entry)?;
    Ok(())
}

pub fn append_assistant_message(
    conn: &rusqlite::Connection,
    session_id: &str,
    model: &Model,
    collected: &CollectedResponse,
) -> Result<()> {
    let mut assistant_content = Vec::new();
    if !collected.thinking.is_empty() {
        assistant_content.push(AssistantContent::Thinking {
            thinking: collected.thinking.clone(),
        });
    }
    if !collected.text.is_empty() {
        assistant_content.push(AssistantContent::Text {
            text: collected.text.clone(),
        });
    }
    for tool_call in &collected.tool_calls {
        let args: serde_json::Value =
            serde_json::from_str(&tool_call.arguments).unwrap_or(serde_json::json!({}));
        assistant_content.push(AssistantContent::ToolCall {
            id: tool_call.id.clone(),
            name: tool_call.name.clone(),
            arguments: args,
        });
    }

    let inp = collected.input_tokens;
    let out = collected.output_tokens;
    let cr = collected.cache_read_tokens;
    let cw = collected.cache_write_tokens;
    let model_cost = &model.cost;
    let cost = Cost {
        input: (model_cost.input / 1_000_000.0) * inp as f64,
        output: (model_cost.output / 1_000_000.0) * out as f64,
        cache_read: (model_cost.cache_read / 1_000_000.0) * cr as f64,
        cache_write: (model_cost.cache_write / 1_000_000.0) * cw as f64,
        total: (model_cost.input / 1_000_000.0) * inp as f64
            + (model_cost.output / 1_000_000.0) * out as f64
            + (model_cost.cache_read / 1_000_000.0) * cr as f64
            + (model_cost.cache_write / 1_000_000.0) * cw as f64,
    };

    let assistant_entry = SessionEntry::Message {
        base: EntryBase {
            id: EntryId::generate(),
            parent_id: get_leaf_raw(conn, session_id),
            timestamp: Utc::now(),
        },
        message: AgentMessage::Assistant(AssistantMessage {
            content: assistant_content,
            provider: model.provider.clone(),
            model: model.id.clone(),
            usage: Usage {
                input: inp,
                output: out,
                cache_read: cr,
                cache_write: cw,
                total_tokens: inp + out + cr + cw,
                cost,
            },
            stop_reason: if collected.tool_calls.is_empty() {
                StopReason::Stop
            } else {
                StopReason::ToolUse
            },
            error_message: None,
            timestamp: Utc::now().timestamp_millis(),
        }),
    };
    store::append_entry(conn, session_id, &assistant_entry)?;
    Ok(())
}

pub async fn execute_tool_calls(
    conn: &Arc<Mutex<rusqlite::Connection>>,
    session_id: &str,
    collected: &CollectedResponse,
    tools: &[Box<dyn Tool>],
    tool_ctx: &ToolContext,
    cancel: &CancellationToken,
    event_tx: &mpsc::UnboundedSender<TurnEvent>,
) -> Result<()> {
    for tool_call in &collected.tool_calls {
        let args: serde_json::Value =
            serde_json::from_str(&tool_call.arguments).unwrap_or(serde_json::json!({}));

        let _ = event_tx.send(TurnEvent::ToolExecuting {
            id: tool_call.id.clone(),
            name: tool_call.name.clone(),
        });

        let tool = tools.iter().find(|tool| tool.name() == tool_call.name);
        let result = match tool {
            Some(tool) => tool.execute(args, tool_ctx, cancel.clone()).await,
            None => Err(bb_core::error::BbError::Tool(format!(
                "Unknown tool: {}",
                tool_call.name
            ))),
        };

        let (content, details, artifact_path, is_error) = match result {
            Ok(r) => (
                r.content,
                r.details,
                r.artifact_path.map(|p| p.display().to_string()),
                r.is_error,
            ),
            Err(e) => (
                vec![ContentBlock::Text {
                    text: format!("Error: {e}"),
                }],
                None,
                None,
                true,
            ),
        };

        let _ = event_tx.send(TurnEvent::ToolResult {
            id: tool_call.id.clone(),
            name: tool_call.name.clone(),
            content: content.clone(),
            details: details.clone(),
            artifact_path: artifact_path.clone(),
            is_error,
        });

        // Lock connection only for the synchronous DB write
        {
            let conn = conn.lock().await;
            let tool_result_entry = SessionEntry::Message {
                base: EntryBase {
                    id: EntryId::generate(),
                    parent_id: get_leaf_raw(&conn, session_id),
                    timestamp: Utc::now(),
                },
                message: AgentMessage::ToolResult(ToolResultMessage {
                    tool_call_id: tool_call.id.clone(),
                    tool_name: tool_call.name.clone(),
                    content,
                    details,
                    is_error,
                    timestamp: Utc::now().timestamp_millis(),
                }),
            };
            store::append_entry(&conn, session_id, &tool_result_entry)?;
        }
    }

    Ok(())
}

pub async fn get_leaf(conn: &Arc<Mutex<rusqlite::Connection>>, session_id: &str) -> Option<EntryId> {
    let conn = conn.lock().await;
    get_leaf_raw(&conn, session_id)
}

/// Get leaf entry using a raw connection reference (for callers that already have one).
pub fn get_leaf_raw(conn: &rusqlite::Connection, session_id: &str) -> Option<EntryId> {
    store::get_session(conn, session_id)
        .ok()
        .flatten()
        .and_then(|s| s.leaf_id.map(EntryId))
}

/// Open a second connection to the same database file, wrapped in `Arc<Mutex<>>`,
/// for use in a spawned task.
pub fn open_sibling_conn(conn: &rusqlite::Connection) -> Result<Arc<Mutex<rusqlite::Connection>>> {
    let path = conn.path().map(|p| std::path::PathBuf::from(p));
    let new_conn = match path {
        Some(p) => store::open_db(&p)?,
        None => store::open_memory()?,
    };
    Ok(Arc::new(Mutex::new(new_conn)))
}

/// Wrap a raw `rusqlite::Connection` in `Arc<Mutex<>>` for use in `TurnConfig`.
pub fn wrap_conn(conn: rusqlite::Connection) -> Arc<Mutex<rusqlite::Connection>> {
    Arc::new(Mutex::new(conn))
}
