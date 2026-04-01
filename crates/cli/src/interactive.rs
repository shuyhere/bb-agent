#[path = "interactive/mod.rs"]
mod controller;

use anyhow::Result;

pub use controller::{InteractiveMode, InteractiveModeOptions, InteractiveResult};

pub async fn run_interactive(cli: crate::Cli) -> Result<()> {
    let options = InteractiveModeOptions {
        verbose: cli.verbose,
        quiet_startup: false,
        migrated_providers: Vec::new(),
        model_fallback_message: None,
        initial_message: cli.messages.first().cloned(),
        initial_images: Vec::new(),
        initial_messages: cli.messages.iter().skip(1).cloned().collect(),
    };

    controller::run_interactive(cli, options)
        .await
        .map_err(|err| anyhow::Error::msg(err.to_string()))
}
