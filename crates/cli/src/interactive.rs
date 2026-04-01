#[path = "interactive/mod.rs"]
mod controller;

use anyhow::Result;
use bb_core::agent_session::{ModelRef, ThinkingLevel, load_agents_md, parse_model_arg};
use bb_core::agent_session_runtime::{
    AgentSessionRuntimeBootstrap, AgentSessionRuntimeHost, CreateAgentSessionRuntimeOptions,
    create_agent_session_runtime,
};
use bb_core::config;
use bb_core::settings::Settings;
use bb_session::store;

pub use controller::{InteractiveMode, InteractiveModeOptions, InteractiveResult};

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
    let session_id = store::create_session(&conn, cwd_str)?;

    // Resolve model via core helper
    let settings = Settings::load_merged(&cwd);
    let model_input = entry.model.as_deref().or(settings.default_model.as_deref());
    let provider_input = entry.provider.as_deref().or(settings.default_provider.as_deref());
    let (provider_name, model_id, thinking_override) = parse_model_arg(provider_input, model_input);

    // Resolve thinking level
    let thinking_str = thinking_override
        .as_deref()
        .or(entry.thinking.as_deref())
        .unwrap_or("off");
    let thinking_level = match thinking_str {
        "low" | "minimal" => ThinkingLevel::Low,
        "medium" => ThinkingLevel::Medium,
        "high" => ThinkingLevel::High,
        "xhigh" => ThinkingLevel::XHigh,
        _ => ThinkingLevel::Off,
    };

    // Load AGENTS.md via core helper
    let agents_md = load_agents_md(&cwd);

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
        session_id: Some(session_id),
        model_display: Some(model_display),
        agents_md,
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

    controller::run_interactive(runtime_host, options)
        .await
        .map_err(|err| anyhow::Error::msg(err.to_string()))
}
