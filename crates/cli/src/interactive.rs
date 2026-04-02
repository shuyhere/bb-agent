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
use bb_provider::anthropic::AnthropicProvider;
use bb_provider::google::GoogleProvider;
use bb_provider::openai::OpenAiProvider;
use bb_provider::registry::{ApiType, ModelRegistry};
use bb_provider::Provider;
use bb_session::store;
use bb_tools::{builtin_tools, Tool, ToolContext};

use crate::login;

pub use controller::{InteractiveMode, InteractiveModeOptions, InteractiveResult, InteractiveSessionSetup};

#[derive(Clone, Debug, Default)]
pub struct InteractiveEntryOptions {
    pub verbose: bool,
    pub messages: Vec<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub thinking: Option<String>,
    pub system_prompt: Option<String>,
    pub append_system_prompt: Option<String>,
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
        }
    }
}

pub async fn run_interactive(entry: InteractiveEntryOptions) -> Result<()> {
    let cwd = std::env::current_dir()?;

    // Use core helpers for session bootstrap
    let global_dir = config::global_dir();
    std::fs::create_dir_all(&global_dir)?;

    // Open session DB
    let conn = store::open_db(&global_dir.join("sessions.db"))?;
    let cwd_str = cwd.to_str().unwrap_or(".");
    // Don't create a session row yet — wait until the first message is sent.
    // This avoids cluttering the DB with empty sessions from bb launches that
    // never send a prompt (like opening bb and immediately closing, or /resume).
    let session_id = uuid::Uuid::new_v4().to_string();

    // Resolve model via core helper
    let settings = Settings::load_merged(&cwd);
    let model_input = entry.model.as_deref().or(settings.default_model.as_deref());
    let provider_input = entry.provider.as_deref().or(settings.default_provider.as_deref());
    let (provider_name, model_id, thinking_override) = parse_model_arg(provider_input, model_input);

    // Resolve thinking level
    let thinking_str = thinking_override
        .as_deref()
        .or(entry.thinking.as_deref())
        .unwrap_or("medium"); // pi default: medium thinking
    let thinking_level = match thinking_str {
        "low" | "minimal" => ThinkingLevel::Low,
        "medium" => ThinkingLevel::Medium,
        "high" => ThinkingLevel::High,
        "xhigh" => ThinkingLevel::XHigh,
        _ => ThinkingLevel::Medium, // pi default
    };

    // Load AGENTS.md via core helper
    let agents_md = load_agents_md(&cwd);

    // Build system prompt (same as run.rs)
    let base_prompt = entry
        .system_prompt
        .as_deref()
        .unwrap_or(DEFAULT_SYSTEM_PROMPT);
    let system_prompt = match &entry.append_system_prompt {
        Some(append) => agent::build_system_prompt(base_prompt, Some(append)),
        None => agent::build_system_prompt(base_prompt, agents_md.as_deref()),
    };

    // Resolve full model via registry (same as run.rs)
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

    // Resolve API key
    let api_key = login::resolve_api_key(&provider_name).unwrap_or_default();

    let base_url = model
        .base_url
        .clone()
        .unwrap_or_else(|| "https://api.openai.com/v1".into());

    // Create provider
    let provider: std::sync::Arc<dyn Provider> = match model.api {
        ApiType::AnthropicMessages => std::sync::Arc::new(AnthropicProvider::new()),
        ApiType::GoogleGenerative => std::sync::Arc::new(GoogleProvider::new()),
        _ => std::sync::Arc::new(OpenAiProvider::new()),
    };

    // Select tools and build definitions
    let tools = select_tools_default();
    let tool_defs = build_tool_defs(&tools);

    // Build tool context
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
        tools,
        tool_defs,
        tool_ctx,
        system_prompt,
        thinking_level: thinking_str.to_string(),
        retry_enabled: settings.retry.enabled,
        retry_max_retries: settings.retry.max_retries,
        retry_base_delay_ms: settings.retry.base_delay_ms,
        session_created: false,
        sibling_conn: None,
    };

    let bootstrap = AgentSessionRuntimeBootstrap {
        cwd: Some(cwd.clone()),
        model: Some(model_ref),
        thinking_level: Some(thinking_level),
        ..AgentSessionRuntimeBootstrap::default()
    };
    let runtime =
        create_agent_session_runtime(&bootstrap, CreateAgentSessionRuntimeOptions::new(cwd));
    let runtime_host = AgentSessionRuntimeHost::new(bootstrap, runtime);

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
