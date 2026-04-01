/// Handle slash commands in interactive mode.
/// Returns a SlashResult indicating what happened.
pub fn handle_slash_command(text: &str) -> SlashResult {
    let text = text.trim();
    match text {
        "/help" => SlashResult::Help,
        "/quit" | "/exit" => SlashResult::Exit,
        "/new" => SlashResult::NewSession,
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
        "/session" => SlashResult::SessionInfo,
        "/settings" => SlashResult::Handled,
        "/name" => SlashResult::Handled,
        cmd if cmd.starts_with("/name ") => {
            let name = cmd.strip_prefix("/name ").unwrap().trim();
            SlashResult::SetName(name.to_string())
        }
        _ => SlashResult::NotCommand,
    }
}

pub enum SlashResult {
    /// Command handled, continue loop
    Handled,
    /// Show help text
    Help,
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
    /// Show session info
    SessionInfo,
    /// Not a slash command — send to LLM
    NotCommand,
}

/// Get help text as lines (for display in TUI).
pub fn help_lines() -> Vec<String> {
    vec![
        "  Available commands:".into(),
        "    /help          Show this help".into(),
        "    /new           Start a new session".into(),
        "    /resume        Resume a previous session".into(),
        "    /model [name]  Switch model".into(),
        "    /compact       Compact conversation context".into(),
        "    /tree          Navigate session tree".into(),
        "    /fork          Fork current session".into(),
        "    /name <name>   Set session display name".into(),
        "    /session       Show current session info".into(),
        "    /login         Login to a provider".into(),
        "    /logout        Logout from a provider".into(),
        "    /settings      Show settings info".into(),
        "    /quit          Exit".into(),
        String::new(),
        "  Shortcuts:".into(),
        "    Ctrl+C         Abort / clear".into(),
        "    Ctrl+D         Exit (empty editor)".into(),
        "    !command       Run bash directly".into(),
    ]
}
