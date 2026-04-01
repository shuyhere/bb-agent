#[path = "interactive/mod.rs"]
mod controller;

use anyhow::Result;

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

    controller::run_interactive(entry, options)
        .await
        .map_err(|err| anyhow::Error::msg(err.to_string()))
}
