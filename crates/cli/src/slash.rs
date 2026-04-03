use bb_tui::slash_commands::shared_slash_command_help_lines;

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
        "/copy" => SlashResult::Copy,
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
    /// Copy last assistant response to clipboard
    Copy,
    /// Not a slash command — send to LLM
    NotCommand,
}

/// Get help text as lines (for display in TUI).
pub fn help_lines() -> Vec<String> {
    shared_slash_command_help_lines()
}

#[cfg(test)]
mod tests {
    use super::{handle_slash_command, SlashResult};

    #[test]
    fn parses_copy_command() {
        assert!(matches!(handle_slash_command("/copy"), SlashResult::Copy));
    }
}
