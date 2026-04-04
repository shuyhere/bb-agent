#[path = "interactive/mod.rs"]
mod controller;

use anyhow::Result;
use bb_core::agent::{self, DEFAULT_SYSTEM_PROMPT};
use bb_core::agent_session::{ModelRef, ThinkingLevel, load_agents_md, parse_model_arg};
use bb_core::agent_session_runtime::{
    AgentSessionRuntimeBootstrap, AgentSessionRuntimeHost, CreateAgentSessionRuntimeOptions,
    create_agent_session_runtime,
};
use bb_core::config;
use bb_core::settings::Settings;
use bb_provider::Provider;
use bb_provider::anthropic::AnthropicProvider;
use bb_provider::google::GoogleProvider;
use bb_provider::openai::OpenAiProvider;
use bb_provider::registry::{ApiType, ModelRegistry};
use bb_session::store;
use bb_tools::{Tool, ToolContext, builtin_tools};

use crate::extensions::{
    ExtensionBootstrap, RuntimeExtensionSupport, auto_install_missing_packages,
    build_skill_system_prompt_section, load_runtime_extension_support_with_ui,
};
use crate::login;

pub use controller::{
    InteractiveModeOptions, InteractiveSessionSetup,
};

#[derive(Clone, Debug, Default)]
pub struct InteractiveEntryOptions {
    pub verbose: bool,
    pub messages: Vec<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub thinking: Option<String>,
    pub system_prompt: Option<String>,
    pub append_system_prompt: Option<String>,
    pub extensions: Vec<String>,
}

impl From<&crate::Cli> for InteractiveEntryOptions {
    fn from(cli: &crate::Cli) -> Self {
        Self {
            verbose: cli.verbose,
            messages: cli.messages.clone(),
            provider: cli.provider.clone(),
            model: cli.model.clone(),
            thinking: cli.thinking.clone(),
            system_prompt: cli.system_prompt.clone(),
            append_system_prompt: cli.append_system_prompt.clone(),
            extensions: cli.extensions.clone(),
        }
    }
}

pub(crate) async fn prepare_interactive_mode(
    entry: InteractiveEntryOptions,
) -> Result<(
    AgentSessionRuntimeHost,
    InteractiveModeOptions,
    InteractiveSessionSetup,
)> {
    let cwd = std::env::current_dir()?;

    let global_dir = config::global_dir();
    std::fs::create_dir_all(&global_dir)?;

    let conn = store::open_db(&global_dir.join("sessions.db"))?;
    let session_id = uuid::Uuid::new_v4().to_string();

    let settings = Settings::load_merged(&cwd);
    let model_input = entry.model.as_deref().or(settings.default_model.as_deref());
    let provider_input = entry
        .provider
        .as_deref()
        .or(settings.default_provider.as_deref());
    let (provider_name, model_id, thinking_override) = parse_model_arg(provider_input, model_input);

    let thinking_str = thinking_override
        .as_deref()
        .or(entry.thinking.as_deref())
        .unwrap_or("medium");
    let thinking_level = match thinking_str {
        "low" | "minimal" => ThinkingLevel::Low,
        "medium" => ThinkingLevel::Medium,
        "high" => ThinkingLevel::High,
        "xhigh" => ThinkingLevel::XHigh,
        _ => ThinkingLevel::Medium,
    };

    let agents_md = load_agents_md(&cwd);

    let base_prompt = entry
        .system_prompt
        .as_deref()
        .unwrap_or(DEFAULT_SYSTEM_PROMPT);
    let system_prompt = match &entry.append_system_prompt {
        Some(append) => agent::build_system_prompt(base_prompt, Some(append)),
        None => agent::build_system_prompt(base_prompt, agents_md.as_deref()),
    };

    let mut registry = ModelRegistry::new();
    registry.load_custom_models(&settings);
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
            base_url: None,
            cost: Default::default(),
        });

    let api_key = login::resolve_api_key(&provider_name).unwrap_or_default();
    let base_url = model
        .base_url
        .clone()
        .unwrap_or_else(|| "https://api.openai.com/v1".into());

    let provider: std::sync::Arc<dyn Provider> = match model.api {
        ApiType::AnthropicMessages => std::sync::Arc::new(AnthropicProvider::new()),
        ApiType::GoogleGenerative => std::sync::Arc::new(GoogleProvider::new()),
        _ => std::sync::Arc::new(OpenAiProvider::new()),
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
    let mut builtin_tools = select_tools_default();
    builtin_tools.append(&mut tools);
    let tool_defs = build_tool_defs(&builtin_tools);
    let skill_section = build_skill_system_prompt_section(&session_resources);
    let system_prompt = format!("{system_prompt}{skill_section}");

    let artifacts_dir = global_dir.join("artifacts");
    std::fs::create_dir_all(&artifacts_dir)?;
    let tool_ctx = ToolContext {
        cwd: cwd.clone(),
        artifacts_dir,
        on_output: None,
    };

    let model_ref = ModelRef {
        provider: provider_name.clone(),
        id: model_id.clone(),
        reasoning: thinking_level != ThinkingLevel::Off,
    };

    let model_display = format!("{}/{}", provider_name, model_id);

    let options = InteractiveModeOptions {
        verbose: entry.verbose,
        quiet_startup: false,
        migrated_providers: Vec::new(),
        model_fallback_message: None,
        initial_message: entry.messages.first().cloned(),
        initial_images: Vec::new(),
        initial_messages: entry.messages.iter().skip(1).cloned().collect(),
        session_id: Some(session_id.clone()),
        model_display: Some(model_display),
        agents_md,
    };

    let setup = InteractiveSessionSetup {
        conn,
        session_id,
        provider,
        model,
        api_key,
        base_url,
        tools: builtin_tools,
        tool_defs,
        tool_ctx,
        system_prompt,
        thinking_level: thinking_str.to_string(),
        retry_enabled: settings.retry.enabled,
        retry_max_retries: settings.retry.max_retries,
        retry_base_delay_ms: settings.retry.base_delay_ms,
        retry_max_delay_ms: settings.retry.max_delay_ms,
        session_created: false,
        sibling_conn: Some(sibling_conn),
        extension_commands: commands,
        extension_bootstrap: extension_bootstrap.clone(),
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
    let runtime_host = AgentSessionRuntimeHost::new(bootstrap, runtime);

    Ok((runtime_host, options, setup))
}

pub async fn run_interactive(entry: InteractiveEntryOptions) -> Result<()> {
    let (runtime_host, options, setup) = prepare_interactive_mode(entry).await?;
    controller::run_interactive(runtime_host, options, setup)
        .await
        .map_err(|err| anyhow::Error::msg(err.to_string()))
}

/// Select all built-in tools (interactive mode always enables all tools).
fn select_tools_default() -> Vec<Box<dyn Tool>> {
    builtin_tools()
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
