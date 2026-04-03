use anyhow::Result;
use clap::{Parser, Subcommand};

mod error;
mod extensions;

#[path = "interactive.rs"]
mod interactive;
mod fullscreen_entry;
mod login;
mod models;
mod oauth;
mod run;
mod slash;
mod turn_runner;

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
  bb --fullscreen-transcript          Shared fullscreen transcript shell
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

    /// Launch the shared fullscreen transcript shell (`--fullscreen` kept as a legacy alias)
    #[arg(long = "fullscreen-transcript", visible_alias = "fullscreen")]
    fullscreen_transcript: bool,

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
    if let Some(cwd) = cli.cwd.as_deref() {
        std::env::set_current_dir(cwd)?;
    }

    // In interactive mode, suppress tracing to avoid leaking into TUI.
    // In print mode or verbose, show warnings.
    let log_level = if cli.verbose {
        tracing::Level::DEBUG
    } else if cli.print {
        tracing::Level::WARN
    } else {
        tracing::Level::ERROR // interactive: only errors, no WARN noise in TUI
    };
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env().add_directive(log_level.into()),
        )
        .with_writer(std::io::stderr)
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

    // Process @file arguments from messages
    let mut cli = cli;
    let mut prompt_parts = Vec::new();
    let mut regular_messages = Vec::new();

    for msg in &cli.messages {
        if msg.starts_with('@') {
            let path = &msg[1..];
            match std::fs::read_to_string(path) {
                Ok(content) => {
                    prompt_parts.push(format!("Contents of {}:\n```\n{}\n```", path, content));
                }
                Err(e) => {
                    eprintln!("Warning: Could not read {}: {}", path, e);
                }
            }
        } else {
            regular_messages.push(msg.clone());
        }
    }

    // Combine file contents with messages
    if !prompt_parts.is_empty() {
        let file_context = prompt_parts.join("\n\n");
        let user_text = regular_messages.join(" ");
        cli.messages = if user_text.is_empty() {
            vec![file_context]
        } else {
            vec![format!("{}\n\n{}", file_context, user_text)]
        };
    }

    let use_fullscreen = fullscreen_transcript_requested(&cli);

    // Print mode stays a thin entry layer; interactive mode owns the legacy TUI controller.
    // The fullscreen entry remains available behind an explicit flag until its UX reaches parity.
    if cli.print {
        run::run_print_mode(cli).await
    } else if use_fullscreen {
        fullscreen_entry::run_fullscreen_entry(interactive::InteractiveEntryOptions::from(&cli))
            .await
    } else {
        interactive::run_interactive(interactive::InteractiveEntryOptions::from(&cli)).await
    }
}

fn fullscreen_transcript_requested(cli: &Cli) -> bool {
    cli.fullscreen_transcript
        || env_flag_enabled("BB_FULLSCREEN_TRANSCRIPT")
        || env_flag_enabled("BB_FULLSCREEN")
}

fn env_flag_enabled(name: &str) -> bool {
    std::env::var(name)
        .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
        .unwrap_or(false)
}

// Cli is already visible to submodules via crate::Cli
