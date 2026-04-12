use anyhow::{Result, anyhow, bail};

use bb_core::agent::{self, DEFAULT_SYSTEM_PROMPT};
use bb_core::agent_session::{
    ImageContent, ModelRef, PrintTurnResult, PrintTurnStopReason, parse_model_arg,
};

use crate::agents_md::load_agents_md;
use bb_core::agent_session_runtime::{
    CreateAgentSessionRuntimeOptions, create_agent_session_runtime,
};
use bb_core::config;
use bb_core::settings::Settings;
use bb_provider::anthropic::AnthropicProvider;
use bb_provider::google::GoogleProvider;
use bb_provider::openai::OpenAiProvider;
use bb_provider::registry::{ApiType, ModelRegistry};
use bb_session::store;
use bb_tools::{ExecutionPolicy, Tool, ToolContext, builtin_tools};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::Cli;
use crate::extensions::{
    ExtensionBootstrap, RuntimeExtensionSupport, auto_install_missing_packages,
    build_skill_system_prompt_section, load_runtime_extension_support,
};
use crate::login;
use crate::turn_runner::{self, TurnConfig, TurnEvent, wrap_conn};

#[derive(Debug, Clone)]
struct PreparedPrintPrompt {
    text: String,
    images: Vec<ImageContent>,
}

pub async fn run_print_mode(cli: Cli) -> Result<()> {
    let cwd = std::fs::canonicalize(cli.cwd.as_deref().unwrap_or("."))?;

    let global_dir = config::global_dir();
    std::fs::create_dir_all(&global_dir)?;
    let artifacts_dir = global_dir.join("artifacts");
    std::fs::create_dir_all(&artifacts_dir)?;

    let conn = store::open_db(&global_dir.join("sessions.db"))?;
    let session_id = resolve_session_id(&conn, &cwd, &cli)?;

    let settings = Settings::load_merged(&cwd);
    let execution_policy = ExecutionPolicy::from(settings.resolved_execution_mode());
    let startup_fallback = crate::login::preferred_startup_provider_and_model(&settings);
    let model_input = cli
        .model
        .as_deref()
        .or(startup_fallback.as_ref().map(|(_, model)| model.as_str()))
        .or(settings.default_model.as_deref());
    let provider_input = cli
        .provider
        .as_deref()
        .or(startup_fallback
            .as_ref()
            .map(|(provider, _)| provider.as_str()))
        .or(settings.default_provider.as_deref());
    let (provider_name, model_id, _thinking_override) =
        parse_model_arg(provider_input, model_input);

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
    login::add_cached_github_copilot_models(&mut registry);
    let model = registry
        .find(&provider_name, &model_id)
        .cloned()
        .or_else(|| {
            registry
                .find_fuzzy(&model_id, Some(&provider_name))
                .cloned()
        })
        .or_else(|| registry.find_fuzzy(&model_id, None).cloned())
        .unwrap_or_else(|| bb_provider::registry::Model {
            id: model_id.clone(),
            name: model_id.clone(),
            provider: provider_name.clone(),
            api: bb_provider::registry::ApiType::OpenaiCompletions,
            context_window: 128_000,
            max_tokens: 16_384,
            reasoning: false,
            input: vec![bb_provider::registry::ModelInput::Text],
            base_url: None,
            cost: Default::default(),
        });

    let api_key = match &cli.api_key {
        Some(key) => key.clone(),
        None => login::resolve_api_key(&provider_name).unwrap_or_default(),
    };
    let base_url = if provider_name == "github-copilot" {
        login::github_copilot_api_base_url()
    } else {
        model
            .base_url
            .clone()
            .unwrap_or_else(|| "https://api.openai.com/v1".into())
    };
    let headers = if provider_name == "github-copilot" {
        login::github_copilot_runtime_headers()
    } else {
        std::collections::HashMap::new()
    };

    auto_install_missing_packages(&cwd, &settings);

    let extension_bootstrap = ExtensionBootstrap::from_cli_values(&cwd, &cli.extensions);
    let RuntimeExtensionSupport {
        session_resources,
        mut tools,
        mut commands,
    } = load_runtime_extension_support(&cwd, &settings, &extension_bootstrap).await?;
    commands.bind_session_context(
        turn_runner::open_sibling_conn(&conn)?,
        session_id.clone(),
        None,
    );
    let _ = commands.send_event(&bb_hooks::Event::SessionStart).await;
    let mut builtin_tools = select_tools(&cli);
    builtin_tools.append(&mut tools);
    let tool_defs = build_tool_defs(&builtin_tools);
    let skill_section = build_skill_system_prompt_section(&session_resources);
    let system_prompt = format!("{system_prompt}{skill_section}");

    let provider: Arc<dyn bb_provider::Provider> = match model.api {
        ApiType::AnthropicMessages => Arc::new(AnthropicProvider::new()),
        ApiType::GoogleGenerative => Arc::new(GoogleProvider::new()),
        _ => Arc::new(OpenAiProvider::new()),
    };

    let tool_ctx = ToolContext {
        cwd: cwd.clone(),
        artifacts_dir,
        execution_policy,
        on_output: None,
        web_search: Some(bb_tools::WebSearchRuntime {
            provider: provider.clone(),
            model: model.clone(),
            api_key: api_key.clone(),
            base_url: base_url.clone(),
            headers: headers.clone(),
            enabled: true,
        }),
        execution_mode: bb_tools::ToolExecutionMode::NonInteractive,
        request_approval: None,
    };

    let bootstrap = bb_core::agent_session_runtime::AgentSessionRuntimeBootstrap {
        cwd: Some(cwd.clone()),
        model: Some(ModelRef {
            provider: provider_name.clone(),
            id: model_id.clone(),
            reasoning: model.reasoning,
        }),
        resource_bootstrap: session_resources,
        ..Default::default()
    };
    let runtime_handle = create_agent_session_runtime(
        &bootstrap,
        CreateAgentSessionRuntimeOptions::new(cwd.clone()),
    );

    let mut prepared_messages = Vec::new();
    for raw in cli.messages {
        if commands.is_registered(&raw) {
            if let Some(output) = commands.execute_text(&raw).await? {
                println!("{output}");
            }
            continue;
        }

        let input = commands.apply_input_hooks(&raw, "interactive").await?;
        if input.handled {
            if let Some(output) = input.output {
                println!("{output}");
            }
            continue;
        }

        if let Some(text) = input.text {
            let expanded_text = runtime_handle.session.expand_input_text(text);
            let expanded = crate::input_files::expand_at_file_references(&expanded_text, &cwd);
            for warning in expanded.warnings {
                eprintln!("Warning: {warning}");
            }
            prepared_messages.push(PreparedPrintPrompt {
                text: expanded.text,
                images: load_images_from_paths(&expanded.image_paths)?,
            });
        }
    }

    let initial_message = if prepared_messages.is_empty() {
        None
    } else {
        Some(prepared_messages.remove(0))
    };
    let follow_up_messages = prepared_messages;

    let turn_config = TurnConfig {
        conn: wrap_conn(conn),
        session_id,
        system_prompt,
        model,
        provider,
        api_key,
        base_url,
        headers,
        compaction_settings: bb_core::types::CompactionSettings {
            enabled: settings.compaction.enabled,
            reserve_tokens: settings.compaction.reserve_tokens,
            keep_recent_tokens: settings.compaction.keep_recent_tokens,
        },
        tools: builtin_tools,
        tool_defs,
        tool_ctx,
        thinking: None,
        retry_enabled: settings.retry.enabled,
        retry_max_retries: settings.retry.max_retries,
        retry_base_delay_ms: settings.retry.base_delay_ms,
        retry_max_delay_ms: settings.retry.max_delay_ms,
        cancel: CancellationToken::new(),
        extensions: commands.clone(),
    };

    let mut last_result = None;
    if let Some(initial_message) = initial_message {
        last_result = Some(run_print_turn(&turn_config, initial_message).await?);
    }
    for message in follow_up_messages {
        last_result = Some(run_print_turn(&turn_config, message).await?);
    }

    let _ = commands.send_event(&bb_hooks::Event::SessionShutdown).await;
    if let Some(last_result) = last_result {
        if last_result.is_error() {
            return Err(anyhow!(last_result.error_message.clone().unwrap_or_else(
                || format!("request {:?}", last_result.stop_reason)
            )));
        }

        if !last_result.text.is_empty() {
            println!("{}", last_result.text);
        }
    }

    Ok(())
}

fn resolve_session_id(
    conn: &rusqlite::Connection,
    cwd: &std::path::Path,
    cli: &Cli,
) -> Result<String> {
    let cwd_str = cwd.to_str().unwrap_or(".");
    if let Some(session_arg) = &cli.session {
        let all = store::list_sessions(conn, cwd_str)?;
        let matches: Vec<_> = all
            .iter()
            .filter(|s| s.session_id.starts_with(session_arg.as_str()))
            .collect();
        return match matches.len() {
            1 => Ok(matches[0].session_id.clone()),
            0 => bail!("No session matching '{}'", session_arg),
            n => bail!("{n} sessions match '{}', be more specific", session_arg),
        };
    }
    if cli.r#continue {
        let sessions = store::list_sessions(conn, cwd_str)?;
        if let Some(s) = sessions.first() {
            tracing::info!("Continuing session {}", s.session_id);
            return Ok(s.session_id.clone());
        }
    }
    store::create_session(conn, cwd_str)
}

fn select_tools(cli: &Cli) -> Vec<Box<dyn Tool>> {
    if cli.no_tools {
        Vec::new()
    } else if let Some(tools_str) = &cli.tools {
        let names: Vec<&str> = tools_str.split(',').map(|s| s.trim()).collect();
        builtin_tools()
            .into_iter()
            .filter(|t| names.contains(&t.name()))
            .collect()
    } else {
        builtin_tools()
    }
}

fn build_tool_defs(tools: &[Box<dyn Tool>]) -> Vec<serde_json::Value> {
    tools
        .iter()
        .map(|t| {
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": t.name(),
                    "description": t.description(),
                    "parameters": t.parameters_schema(),
                }
            })
        })
        .collect()
}

fn load_images_from_paths(paths: &[std::path::PathBuf]) -> Result<Vec<ImageContent>> {
    use base64::Engine;

    let mut images = Vec::new();
    for path in paths {
        let data = std::fs::read(path)
            .map_err(|error| anyhow!("Could not read image {}: {error}", path.display()))?;
        let Some(mime_type) = image_mime_type(path) else {
            continue;
        };
        images.push(ImageContent {
            source: base64::engine::general_purpose::STANDARD.encode(data),
            mime_type: Some(mime_type.to_string()),
        });
    }
    Ok(images)
}

fn image_mime_type(path: &std::path::Path) -> Option<&'static str> {
    match path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => Some("image/png"),
        Some("jpg" | "jpeg") => Some("image/jpeg"),
        Some("gif") => Some("image/gif"),
        Some("webp") => Some("image/webp"),
        _ => None,
    }
}

async fn run_print_turn(
    config: &TurnConfig,
    prompt: PreparedPrintPrompt,
) -> Result<PrintTurnResult> {
    if !prompt.images.is_empty() && !config.model.supports_images() {
        eprintln!(
            "Warning: model '{}' does not advertise image input support. Attached images may be ignored.",
            config.model.id
        );
    }
    turn_runner::append_user_message_with_images(
        &config.conn,
        &config.session_id,
        &prompt.text,
        &prompt.images,
    )
    .await?;

    let (event_tx, mut event_rx) = mpsc::unbounded_channel();

    // Run the turn loop directly (print mode is single-threaded, no need to spawn).
    turn_runner::run_turn_inner(config, &event_tx, &prompt.text).await?;
    drop(event_tx);

    // Drain remaining events
    let mut final_text = String::new();
    let mut error_message = None;
    while let Some(event) = event_rx.recv().await {
        match event {
            TurnEvent::Done { text } => {
                final_text = text;
            }
            TurnEvent::Error(msg) => {
                error_message = Some(msg);
            }
            _ => {}
        }
    }

    if let Some(err) = &error_message {
        Ok(PrintTurnResult {
            text: final_text,
            stop_reason: PrintTurnStopReason::Error,
            error_message: Some(err.clone()),
        })
    } else {
        Ok(PrintTurnResult {
            text: final_text,
            stop_reason: PrintTurnStopReason::Completed,
            error_message: None,
        })
    }
}
