#[path = "interactive/mod.rs"]
mod controller;

use anyhow::Result;
use bb_core::agent_session::{ModelRef, ThinkingLevel};
use bb_core::agent_session_runtime::{
    AgentSessionRuntimeBootstrap, AgentSessionRuntimeHost, CreateAgentSessionRuntimeOptions,
    create_agent_session_runtime,
};

pub use controller::{InteractiveMode, InteractiveModeOptions, InteractiveResult};

#[derive(Clone, Debug, Default)]
pub struct InteractiveEntryOptions {
    pub verbose: bool,
    pub messages: Vec<String>,
}

impl From<&crate::Cli> for InteractiveEntryOptions {
    fn from(cli: &crate::Cli) -> Self {
        Self {
            verbose: cli.verbose,
            messages: cli.messages.clone(),
        }
    }
}

fn default_interactive_model() -> ModelRef {
    ModelRef {
        provider: "openai".to_string(),
        id: "gpt-5.4".to_string(),
        reasoning: false,
    }
}

pub async fn run_interactive(entry: InteractiveEntryOptions) -> Result<()> {
    let options = InteractiveModeOptions {
        verbose: entry.verbose,
        quiet_startup: false,
        migrated_providers: Vec::new(),
        model_fallback_message: None,
        initial_message: entry.messages.first().cloned(),
        initial_images: Vec::new(),
        initial_messages: entry.messages.iter().skip(1).cloned().collect(),
    };

    let cwd = std::env::current_dir()?;
    let bootstrap = AgentSessionRuntimeBootstrap {
        cwd: Some(cwd.clone()),
        model: Some(default_interactive_model()),
        thinking_level: Some(ThinkingLevel::Off),
        ..AgentSessionRuntimeBootstrap::default()
    };
    let runtime =
        create_agent_session_runtime(&bootstrap, CreateAgentSessionRuntimeOptions::new(cwd));
    let runtime_host = AgentSessionRuntimeHost::new(bootstrap, runtime);

    controller::run_interactive(runtime_host, options)
        .await
        .map_err(|err| anyhow::Error::msg(err.to_string()))
}
