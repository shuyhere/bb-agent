use anyhow::Result;
use bb_core::agent::{self, DEFAULT_SYSTEM_PROMPT};
use bb_core::agent_session::{ModelRef, ThinkingLevel, parse_model_arg};
use bb_core::types::SessionContext;

use crate::agents_md::load_agents_md;
use bb_core::agent_session_runtime::{
    AgentSessionRuntimeBootstrap, AgentSessionRuntimeHost, CreateAgentSessionRuntimeOptions,
    RuntimeModelRef, create_agent_session_runtime,
};
use bb_core::config;
use bb_core::settings::Settings;
use bb_provider::Provider;
use bb_provider::anthropic::AnthropicProvider;
use bb_provider::google::GoogleProvider;
use bb_provider::openai::OpenAiProvider;
use bb_provider::registry::{ApiType, ModelRegistry};
use bb_session::store;
use bb_tools::{ExecutionPolicy, Tool, ToolContext, builtin_tools};
use std::sync::Arc;

use crate::extensions::{
    ExtensionBootstrap, ExtensionCommandRegistry, RuntimeExtensionSupport,
    auto_install_missing_packages, build_skill_system_prompt_section,
    load_runtime_extension_support_with_ui,
};
use crate::login;

#[derive(Clone, Debug, Default)]
pub struct SessionBootstrapOptions {
    pub messages: Vec<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub thinking: Option<String>,
    pub system_prompt: Option<String>,
    pub append_system_prompt: Option<String>,
    pub extensions: Vec<String>,
    pub session: Option<String>,
    pub continue_session: bool,
    pub resume: bool,
    /// Label for the active system prompt (template name, "custom", or "default").
    pub prompt_label: String,
}

impl From<&crate::Cli> for SessionBootstrapOptions {
    fn from(cli: &crate::Cli) -> Self {
        let prompt_label = if cli.system_prompt_template.is_some() {
            cli.system_prompt_template.clone().unwrap_or_default()
        } else if cli.system_prompt.is_some() {
            "custom".to_string()
        } else if cli.append_system_prompt.is_some() {
            "default+append".to_string()
        } else {
            "default".to_string()
        };
        Self {
            messages: cli.messages.clone(),
            provider: cli.provider.clone(),
            model: cli.model.clone(),
            thinking: cli.thinking.clone(),
            system_prompt: cli.system_prompt.clone(),
            append_system_prompt: cli.append_system_prompt.clone(),
            extensions: cli.extensions.clone(),
            session: cli.session.clone(),
            continue_session: cli.r#continue,
            resume: cli.resume,
            prompt_label,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct SessionUiOptions {
    pub initial_message: Option<String>,
    pub initial_messages: Vec<String>,
    pub session_id: Option<String>,
    pub model_display: Option<String>,
    /// Label for the active system prompt template.
    pub prompt_label: String,
}

/// Non-clone runtime state needed for actual LLM calls.
pub struct SessionRuntimeSetup {
    pub conn: rusqlite::Connection,
    pub session_id: String,
    pub provider: Arc<dyn Provider>,
    pub model: bb_provider::registry::Model,
    pub api_key: String,
    pub base_url: String,
    pub headers: std::collections::HashMap<String, String>,
    pub tools: Vec<Box<dyn Tool>>,
    pub tool_defs: Vec<serde_json::Value>,
    pub tool_ctx: ToolContext,
    pub system_prompt: String,
    pub base_system_prompt: String,
    pub thinking_level: String,
    pub compaction_enabled: bool,
    pub compaction_reserve_tokens: u64,
    pub compaction_keep_recent_tokens: u64,
    pub retry_enabled: bool,
    pub retry_max_retries: u32,
    pub retry_base_delay_ms: u64,
    pub retry_max_delay_ms: u64,
    /// Whether the session row has been created in the DB yet.
    pub session_created: bool,
    /// Cached sibling DB connection for the turn runner (avoid opening a new one each turn).
    pub sibling_conn: Option<std::sync::Arc<tokio::sync::Mutex<rusqlite::Connection>>>,
    pub extension_commands: ExtensionCommandRegistry,
    pub extension_bootstrap: ExtensionBootstrap,
}

fn load_resumed_session_context(
    conn: &rusqlite::Connection,
    session_id: &str,
    session_created: bool,
) -> Option<SessionContext> {
    if !session_created {
        return None;
    }
    bb_session::context::build_context(conn, session_id).ok()
}

pub(crate) async fn prepare_session_runtime(
    entry: SessionBootstrapOptions,
) -> Result<(
    AgentSessionRuntimeHost,
    SessionUiOptions,
    SessionRuntimeSetup,
)> {
    let cwd = std::env::current_dir()?;

    let global_dir = config::global_dir();
    std::fs::create_dir_all(&global_dir)?;

    let conn = store::open_db(&global_dir.join("sessions.db"))?;
    let (session_id, session_created) = resolve_startup_session_id(&conn, &cwd, &entry)?;

    let settings = Settings::load_merged(&cwd);
    let execution_policy = ExecutionPolicy::from(settings.resolved_execution_mode());
    let startup_fallback = crate::login::preferred_startup_provider_and_model(&settings);
    let resumed_session_context = load_resumed_session_context(&conn, &session_id, session_created);
    let resumed_model = resumed_session_context
        .as_ref()
        .and_then(|ctx| ctx.model.as_ref());
    let model_input = entry
        .model
        .as_deref()
        .or(resumed_model.map(|model| model.model_id.as_str()))
        .or(startup_fallback.as_ref().map(|(_, model)| model.as_str()))
        .or(settings.default_model.as_deref());
    let provider_input = entry
        .provider
        .as_deref()
        .or(resumed_model.map(|model| model.provider.as_str()))
        .or(startup_fallback
            .as_ref()
            .map(|(provider, _)| provider.as_str()))
        .or(settings.default_provider.as_deref());
    let (provider_name, model_id, thinking_override) = parse_model_arg(provider_input, model_input);

    let requested_thinking = thinking_override
        .as_deref()
        .or(entry.thinking.as_deref())
        .or(resumed_session_context
            .as_ref()
            .map(|ctx| ctx.thinking_level.as_str()))
        .unwrap_or("medium");
    let thinking_level = ThinkingLevel::parse(requested_thinking).unwrap_or(ThinkingLevel::Medium);
    let thinking_str = thinking_level.as_str();

    let agents_md = load_agents_md(&cwd);

    let base_prompt = entry
        .system_prompt
        .as_deref()
        .unwrap_or(DEFAULT_SYSTEM_PROMPT);
    let base_system_prompt = match &entry.append_system_prompt {
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

    let api_key = login::resolve_api_key(&provider_name).unwrap_or_default();
    let base_url = if provider_name == "github-copilot" {
        crate::login::github_copilot_api_base_url()
    } else {
        model
            .base_url
            .clone()
            .unwrap_or_else(|| "https://api.openai.com/v1".into())
    };

    let provider: Arc<dyn Provider> = match model.api {
        ApiType::AnthropicMessages => Arc::new(AnthropicProvider::new()),
        ApiType::GoogleGenerative => Arc::new(GoogleProvider::new()),
        _ => Arc::new(OpenAiProvider::new()),
    };
    let headers = if provider_name == "github-copilot" {
        login::github_copilot_runtime_headers()
    } else {
        std::collections::HashMap::new()
    };

    auto_install_missing_packages(&cwd, &settings);

    let extension_bootstrap = ExtensionBootstrap::from_cli_values(&cwd, &entry.extensions);
    let RuntimeExtensionSupport {
        session_resources,
        mut tools,
        mut commands,
    } = load_runtime_extension_support_with_ui(&cwd, &settings, &extension_bootstrap, true).await?;
    let sibling_conn = crate::turn_runner::open_sibling_conn(&conn)?;
    commands.bind_session_context(sibling_conn.clone(), session_id.clone(), None);
    let _ = commands.send_event(&bb_hooks::Event::SessionStart).await;
    let mut builtin_tools = builtin_tools();
    builtin_tools.append(&mut tools);
    let tool_defs = build_tool_defs(&builtin_tools);
    let skill_section = build_skill_system_prompt_section(&session_resources);
    let system_prompt = format!("{base_system_prompt}{skill_section}");

    let artifacts_dir = global_dir.join("artifacts");
    std::fs::create_dir_all(&artifacts_dir)?;
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
        execution_mode: bb_tools::ToolExecutionMode::Interactive,
        request_approval: None,
    };

    let model_ref = ModelRef {
        provider: provider_name.clone(),
        id: model_id.clone(),
        reasoning: thinking_level.reasoning_enabled(),
    };

    let model_display = format!("{}/{}", provider_name, model_id);

    let options = SessionUiOptions {
        initial_message: entry.messages.first().cloned(),
        initial_messages: entry.messages.iter().skip(1).cloned().collect(),
        session_id: Some(session_id.clone()),
        model_display: Some(model_display),
        prompt_label: entry.prompt_label.clone(),
    };

    let runtime_model = RuntimeModelRef {
        provider: model.provider.clone(),
        id: model.id.clone(),
        context_window: model.context_window as usize,
    };

    let setup = SessionRuntimeSetup {
        conn,
        session_id,
        provider,
        model,
        api_key,
        base_url,
        headers,
        tools: builtin_tools,
        tool_defs,
        tool_ctx,
        system_prompt,
        base_system_prompt,
        thinking_level: thinking_str.to_string(),
        compaction_enabled: settings.compaction.enabled,
        compaction_reserve_tokens: settings.compaction.reserve_tokens,
        compaction_keep_recent_tokens: settings.compaction.keep_recent_tokens,
        retry_enabled: settings.retry.enabled,
        retry_max_retries: settings.retry.max_retries,
        retry_base_delay_ms: settings.retry.base_delay_ms,
        retry_max_delay_ms: settings.retry.max_delay_ms,
        session_created,
        sibling_conn: Some(sibling_conn),
        extension_commands: commands,
        extension_bootstrap,
    };

    let bootstrap = AgentSessionRuntimeBootstrap {
        cwd: Some(cwd.clone()),
        model: Some(model_ref),
        thinking_level: Some(thinking_level),
        resource_bootstrap: session_resources,
        ..AgentSessionRuntimeBootstrap::default()
    };
    let runtime =
        create_agent_session_runtime(&bootstrap, CreateAgentSessionRuntimeOptions::new(cwd));
    let mut runtime_host = AgentSessionRuntimeHost::new(bootstrap, runtime);
    runtime_host.runtime_mut().model = Some(runtime_model);

    Ok((runtime_host, options, setup))
}

fn resolve_startup_session_id(
    conn: &rusqlite::Connection,
    cwd: &std::path::Path,
    entry: &SessionBootstrapOptions,
) -> Result<(String, bool)> {
    let cwd_str = cwd.to_str().unwrap_or(".");

    if let Some(session_arg) = &entry.session {
        let all = store::list_sessions(conn, cwd_str)?;
        let matches: Vec<_> = all
            .iter()
            .filter(|s| s.session_id.starts_with(session_arg.as_str()))
            .collect();
        return match matches.len() {
            1 => Ok((matches[0].session_id.clone(), true)),
            0 => anyhow::bail!("No session matching '{}'", session_arg),
            n => anyhow::bail!("{n} sessions match '{}', be more specific", session_arg),
        };
    }

    if entry.continue_session || entry.resume {
        let sessions = store::list_sessions(conn, cwd_str)?;
        if let Some(session) = sessions.first() {
            return Ok((session.session_id.clone(), true));
        }
    }

    Ok((uuid::Uuid::new_v4().to_string(), false))
}

pub(crate) fn build_tool_defs(tools: &[Box<dyn Tool>]) -> Vec<serde_json::Value> {
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
