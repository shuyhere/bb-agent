use anyhow::{anyhow, bail, Result};

use bb_core::agent::{self, DEFAULT_SYSTEM_PROMPT};
use bb_core::agent_session::{
    load_agents_md, messages_to_provider, parse_model_arg, PrintTurnResult, PrintTurnStopReason,
    ThinPrintSession,
};
use bb_core::config;
use bb_core::settings::Settings;
use bb_core::types::*;
use bb_provider::anthropic::AnthropicProvider;
use bb_provider::google::GoogleProvider;
use bb_provider::openai::OpenAiProvider;
use bb_provider::registry::{ApiType, ModelRegistry};
use bb_provider::streaming::CollectedResponse;
use bb_provider::{CompletionRequest, Provider, RequestOptions};
use bb_session::{context, store};
use bb_tools::{builtin_tools, Tool, ToolContext};
use chrono::Utc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::login;
use crate::Cli;

pub async fn run_print_mode(cli: Cli) -> Result<()> {
    let cwd = std::fs::canonicalize(cli.cwd.as_deref().unwrap_or("."))?;

    let global_dir = config::global_dir();
    std::fs::create_dir_all(&global_dir)?;
    let artifacts_dir = global_dir.join("artifacts");
    std::fs::create_dir_all(&artifacts_dir)?;

    let conn = store::open_db(&global_dir.join("sessions.db"))?;
    let session_id = resolve_session_id(&conn, &cwd, &cli)?;

    let settings = Settings::load_merged(&cwd);
    let model_input = cli.model.as_deref().or(settings.default_model.as_deref());
    let provider_input = cli.provider.as_deref().or(settings.default_provider.as_deref());
    let (provider_name, model_id, _thinking_override) = parse_model_arg(provider_input, model_input);

    let agents_md = load_agents_md(&cwd);
    let base_prompt = cli
        .system_prompt
        .as_deref()
        .unwrap_or(DEFAULT_SYSTEM_PROMPT);
    let system_prompt = match &cli.append_system_prompt {
        Some(append) => agent::build_system_prompt(base_prompt, Some(append)),
        None => agent::build_system_prompt(base_prompt, agents_md.as_deref()),
    };

    let mut registry = ModelRegistry::new();
    registry.load_custom_models(&settings);
    let model = registry
        .find(&provider_name, &model_id)
        .cloned()
        .or_else(|| registry.find_fuzzy(&model_id, Some(&provider_name)).cloned())
        .or_else(|| registry.find_fuzzy(&model_id, None).cloned())
        .unwrap_or_else(|| bb_provider::registry::Model {
            id: model_id.clone(),
            name: model_id.clone(),
            provider: provider_name.clone(),
            api: bb_provider::registry::ApiType::OpenaiCompletions,
            context_window: 128_000,
            max_tokens: 16_384,
            reasoning: false,
            base_url: None,
            cost: Default::default(),
        });

    let api_key = match &cli.api_key {
        Some(key) => key.clone(),
        None => login::resolve_api_key(&provider_name).unwrap_or_default(),
    };
    let base_url = model
        .base_url
        .clone()
        .unwrap_or_else(|| "https://api.openai.com/v1".into());

    let tools = select_tools(&cli);
    let tool_defs = build_tool_defs(&tools);
    let tool_ctx = ToolContext {
        cwd: cwd.clone(),
        artifacts_dir,
        on_output: None,
    };

    let provider: Box<dyn Provider> = match model.api {
        ApiType::AnthropicMessages => Box::new(AnthropicProvider::new()),
        ApiType::GoogleGenerative => Box::new(GoogleProvider::new()),
        _ => Box::new(OpenAiProvider::new()),
    };

    let initial_message = if cli.messages.is_empty() {
        None
    } else {
        Some(cli.messages.join(" "))
    };
    let follow_up_messages = Vec::new();

    let mut session = ThinPrintSession::new(|prompt: String| {
        run_print_turn(
            &conn,
            &session_id,
            prompt,
            &system_prompt,
            &model,
            &*provider,
            &api_key,
            &base_url,
            &tools,
            &tool_defs,
            &tool_ctx,
        )
    });

    let last_result = session.run(initial_message, follow_up_messages).await?;
    if let Some(last_result) = last_result {
        if last_result.is_error() {
            return Err(anyhow!(
                last_result
                    .error_message
                    .clone()
                    .unwrap_or_else(|| format!("request {:?}", last_result.stop_reason))
            ));
        }

        if !last_result.text.is_empty() {
            println!("{}", last_result.text);
        }
    }

    Ok(())
}

fn resolve_session_id(conn: &rusqlite::Connection, cwd: &std::path::Path, cli: &Cli) -> Result<String> {
    let cwd_str = cwd.to_str().unwrap_or(".");
    if let Some(session_arg) = &cli.session {
        let all_sessions = store::list_sessions(conn, cwd_str)?;
        let matches: Vec<_> = all_sessions
            .iter()
            .filter(|session| session.session_id.starts_with(session_arg.as_str()))
            .collect();
        return match matches.len() {
            1 => Ok(matches[0].session_id.clone()),
            0 => bail!("No session matching '{}'", session_arg),
            n => bail!("{n} sessions match '{}', be more specific", session_arg),
        };
    }

    if cli.r#continue {
        let sessions = store::list_sessions(conn, cwd_str)?;
        if let Some(session) = sessions.first() {
            tracing::info!("Continuing session {}", session.session_id);
            return Ok(session.session_id.clone());
        }
    }

    store::create_session(conn, cwd_str).map_err(Into::into)
}

fn select_tools(cli: &Cli) -> Vec<Box<dyn Tool>> {
    if cli.no_tools {
        Vec::new()
    } else if let Some(tools_str) = &cli.tools {
        let tool_names: Vec<&str> = tools_str.split(',').map(|name| name.trim()).collect();
        builtin_tools()
            .into_iter()
            .filter(|tool| tool_names.contains(&tool.name()))
            .collect()
    } else {
        builtin_tools()
    }
}

fn build_tool_defs(tools: &[Box<dyn Tool>]) -> Vec<serde_json::Value> {
    tools
        .iter()
        .map(|tool| {
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": tool.name(),
                    "description": tool.description(),
                    "parameters": tool.parameters_schema(),
                }
            })
        })
        .collect()
}

async fn run_print_turn(
    conn: &rusqlite::Connection,
    session_id: &str,
    prompt: String,
    system_prompt: &str,
    model: &bb_provider::registry::Model,
    provider: &dyn Provider,
    api_key: &str,
    base_url: &str,
    tools: &[Box<dyn Tool>],
    tool_defs: &[serde_json::Value],
    tool_ctx: &ToolContext,
) -> Result<PrintTurnResult> {
    append_user_message(conn, session_id, &prompt)?;

    loop {
        let ctx = context::build_context(conn, session_id)?;
        let request = CompletionRequest {
            system_prompt: system_prompt.to_string(),
            messages: messages_to_provider(&ctx.messages),
            tools: tool_defs.to_vec(),
            model: model.id.clone(),
            max_tokens: Some(model.max_tokens as u32),
            stream: true,
            thinking: None,
        };
        let options = RequestOptions {
            api_key: api_key.to_string(),
            base_url: base_url.to_string(),
            headers: std::collections::HashMap::new(),
            cancel: CancellationToken::new(),
        };

        let (tx, mut rx) = mpsc::unbounded_channel();
        provider.stream(request, options, tx).await?;

        let mut all_events = Vec::new();
        while let Some(event) = rx.recv().await {
            all_events.push(event);
        }

        let collected = CollectedResponse::from_events(&all_events);
        append_assistant_message(conn, session_id, model, &collected)?;

        if collected.tool_calls.is_empty() {
            return Ok(PrintTurnResult {
                text: collected.text,
                stop_reason: PrintTurnStopReason::Completed,
                error_message: None,
            });
        }

        execute_tool_calls(conn, session_id, &collected, tools, tool_ctx).await?;
    }
}

fn append_user_message(conn: &rusqlite::Connection, session_id: &str, prompt: &str) -> Result<()> {
    let user_entry = SessionEntry::Message {
        base: EntryBase {
            id: EntryId::generate(),
            parent_id: get_leaf(conn, session_id),
            timestamp: Utc::now(),
        },
        message: AgentMessage::User(UserMessage {
            content: vec![ContentBlock::Text {
                text: prompt.to_string(),
            }],
            timestamp: Utc::now().timestamp_millis(),
        }),
    };
    store::append_entry(conn, session_id, &user_entry)?;
    Ok(())
}

fn append_assistant_message(
    conn: &rusqlite::Connection,
    session_id: &str,
    model: &bb_provider::registry::Model,
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

    let assistant_entry = SessionEntry::Message {
        base: EntryBase {
            id: EntryId::generate(),
            parent_id: get_leaf(conn, session_id),
            timestamp: Utc::now(),
        },
        message: AgentMessage::Assistant(AssistantMessage {
            content: assistant_content,
            provider: model.provider.clone(),
            model: model.id.clone(),
            usage: Usage {
                input: collected.input_tokens,
                output: collected.output_tokens,
                ..Default::default()
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

async fn execute_tool_calls(
    conn: &rusqlite::Connection,
    session_id: &str,
    collected: &CollectedResponse,
    tools: &[Box<dyn Tool>],
    tool_ctx: &ToolContext,
) -> Result<()> {
    let cancel = CancellationToken::new();

    for tool_call in &collected.tool_calls {
        let args: serde_json::Value =
            serde_json::from_str(&tool_call.arguments).unwrap_or(serde_json::json!({}));
        let tool = tools.iter().find(|tool| tool.name() == tool_call.name);
        let result = match tool {
            Some(tool) => tool.execute(args, tool_ctx, cancel.clone()).await,
            None => Err(bb_core::error::BbError::Tool(format!(
                "Unknown tool: {}",
                tool_call.name
            ))),
        };

        let (content, is_error) = match result {
            Ok(result) => (result.content, result.is_error),
            Err(err) => (
                vec![ContentBlock::Text {
                    text: format!("Error: {err}"),
                }],
                true,
            ),
        };

        let tool_result_entry = SessionEntry::Message {
            base: EntryBase {
                id: EntryId::generate(),
                parent_id: get_leaf(conn, session_id),
                timestamp: Utc::now(),
            },
            message: AgentMessage::ToolResult(ToolResultMessage {
                tool_call_id: tool_call.id.clone(),
                tool_name: tool_call.name.clone(),
                content,
                details: None,
                is_error,
                timestamp: Utc::now().timestamp_millis(),
            }),
        };
        store::append_entry(conn, session_id, &tool_result_entry)?;
    }

    Ok(())
}

fn get_leaf(conn: &rusqlite::Connection, session_id: &str) -> Option<EntryId> {
    store::get_session(conn, session_id)
        .ok()
        .flatten()
        .and_then(|session| session.leaf_id.map(EntryId))
}
