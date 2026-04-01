/// Handle slash commands in interactive mode.
/// Returns true if the command was handled (don't send to LLM).
pub fn handle_slash_command(text: &str) -> SlashResult {
    let text = text.trim();
    match text {
        "/help" => {
            print_help();
            SlashResult::Handled
        }
        "/quit" | "/exit" => SlashResult::Exit,
        "/new" => {
            println!("Starting new session...");
            SlashResult::NewSession
        }
        "/compact" => SlashResult::Compact(None),
        cmd if cmd.starts_with("/compact ") => {
            let instructions = cmd.strip_prefix("/compact ").unwrap().trim();
            SlashResult::Compact(Some(instructions.to_string()))
        }
        "/model" => SlashResult::ModelSelect(None),
        cmd if cmd.starts_with("/model ") => {
            let search = cmd.strip_prefix("/model ").unwrap().trim();
            SlashResult::ModelSelect(Some(search.to_string()))
        }
        "/resume" => SlashResult::Resume,
        "/tree" => SlashResult::Tree,
        "/fork" => SlashResult::Fork,
        "/login" => SlashResult::Login,
        "/logout" => SlashResult::Logout,
        "/settings" => {
            println!("Settings: ~/.bb-agent/settings.json");
            SlashResult::Handled
        }
        "/name" => {
            println!("Usage: /name <session-name>");
            SlashResult::Handled
        }
        cmd if cmd.starts_with("/name ") => {
            let name = cmd.strip_prefix("/name ").unwrap().trim();
            SlashResult::SetName(name.to_string())
        }
        _ => {
            println!("Unknown command: {text}");
            println!("Type /help for available commands.");
            SlashResult::Handled
        }
    }
}

pub enum SlashResult {
    /// Command handled, continue loop
    Handled,
    /// Exit the agent
    Exit,
    /// Start new session
    NewSession,
    /// Compact context
    Compact(Option<String>),
    /// Show model selector
    ModelSelect(Option<String>),
    /// Resume a previous session
    Resume,
    /// Show tree navigation
    Tree,
    /// Fork current session
    Fork,
    /// Login to provider
    Login,
    /// Logout from provider
    Logout,
    /// Set session name
    SetName(String),
    /// Not a slash command — send to LLM
    NotCommand,
}

fn print_help() {
    println!("Available commands:");
    println!("  /help          Show this help");
    println!("  /new           Start a new session");
    println!("  /resume        Resume a previous session");
    println!("  /model [name]  Switch model");
    println!("  /compact       Compact conversation context");
    println!("  /tree          Navigate session tree");
    println!("  /fork          Fork current session");
    println!("  /name <name>   Set session display name");
    println!("  /login         Login to a provider");
    println!("  /logout        Logout from a provider");
    println!("  /settings      Show settings info");
    println!("  /quit          Exit");
    println!();
    println!("Shortcuts:");
    println!("  Ctrl+C         Abort / clear");
    println!("  Ctrl+D         Exit (empty editor)");
    println!("  !command       Run bash directly");
    println!("  !!command      Run bash (no context)");
}
