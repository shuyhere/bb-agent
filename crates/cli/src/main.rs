use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod agent_loop;
mod login;
mod models;
mod run;
mod session;
mod slash;

#[derive(Parser)]
#[command(
    name = "bb",
    about = "BB-Agent — a minimal Rust-native coding agent",
    version,
    after_help = r#"Examples:
  bb                                  Interactive mode
  bb "List all .rs files in src/"     Interactive with initial prompt
  bb -p "What is 2+2?"               Print mode (non-interactive)
  bb -c                               Continue previous session
  bb -r                               Resume: pick a session
  bb --model anthropic/claude-sonnet-4-20250514
  bb --model sonnet:high              Model with thinking level
  bb --list-models                    List all available models
  bb --list-models sonnet             Search models
  bb login                            Login to a provider (OAuth)
  bb logout                           Logout from a provider"#
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Working directory
    #[arg(short = 'C', long, global = true)]
    cwd: Option<String>,

    /// Provider name (or use --model provider/model)
    #[arg(long)]
    provider: Option<String>,

    /// Model ID or "provider/model" or "model:thinking"
    #[arg(long)]
    model: Option<String>,

    /// API key (defaults to env vars)
    #[arg(long)]
    api_key: Option<String>,

    /// System prompt override
    #[arg(long)]
    system_prompt: Option<String>,

    /// Append to system prompt
    #[arg(long)]
    append_system_prompt: Option<String>,

    /// Thinking level: off, low, medium, high
    #[arg(long)]
    thinking: Option<String>,

    /// Non-interactive mode: process prompt and exit
    #[arg(short, long)]
    print: bool,

    /// Continue previous session
    #[arg(short, long)]
    r#continue: bool,

    /// Resume: select a session to continue
    #[arg(short, long)]
    resume: bool,

    /// Don't save session (ephemeral)
    #[arg(long)]
    no_session: bool,

    /// Use specific session file/ID
    #[arg(long)]
    session: Option<String>,

    /// Comma-separated tools to enable (default: read,bash,edit,write)
    #[arg(long)]
    tools: Option<String>,

    /// Disable all tools
    #[arg(long)]
    no_tools: bool,

    /// List available models (with optional search)
    #[arg(long)]
    list_models: Option<Option<String>>,

    /// Models for Ctrl+P cycling (comma-separated patterns)
    #[arg(long)]
    models: Option<String>,

    /// Load a plugin file
    #[arg(short = 'e', long = "extension")]
    extensions: Vec<String>,

    /// Verbose startup
    #[arg(long)]
    verbose: bool,

    /// Initial prompt / messages
    #[arg(trailing_var_arg = true)]
    messages: Vec<String>,
}

#[derive(Subcommand)]
enum Commands {
    /// Login to a provider (OAuth / API key)
    Login {
        /// Provider name (anthropic, openai, google, ...)
        provider: Option<String>,
    },
    /// Logout from a provider
    Logout {
        /// Provider name
        provider: Option<String>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env().add_directive(
                if cli.verbose {
                    tracing::Level::DEBUG
                } else {
                    tracing::Level::WARN
                }
                .into(),
            ),
        )
        .init();

    // Handle subcommands
    if let Some(cmd) = &cli.command {
        return match cmd {
            Commands::Login { provider } => login::handle_login(provider.as_deref()).await,
            Commands::Logout { provider } => login::handle_logout(provider.as_deref()).await,
        };
    }

    // Handle --list-models
    if let Some(search) = &cli.list_models {
        let search_term = match search {
            Some(s) => Some(s.as_str()),
            None => {
                // Check if first message looks like a search term
                if !cli.messages.is_empty() && !cli.messages[0].contains(' ') {
                    Some(cli.messages[0].as_str())
                } else {
                    None
                }
            }
        };
        models::list_models(search_term);
        return Ok(());
    }

    // Run the agent
    run::run_agent(cli).await
}

// Cli is already visible to submodules via crate::Cli
