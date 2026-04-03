use anyhow::Result;
use bb_tui::slash_commands::shared_slash_command_help_lines;

/// Handle shared local slash commands.
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
        "/settings" => SlashResult::Settings,
        "/name" => SlashResult::Name(None),
        cmd if cmd.starts_with("/name ") => {
            let name = cmd.strip_prefix("/name ").unwrap().trim();
            SlashResult::Name(Some(name.to_string()))
        }
        _ => SlashResult::NotCommand,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlashResult {
    Help,
    Exit,
    NewSession,
    Compact(Option<String>),
    ModelSelect(Option<String>),
    Resume,
    Tree,
    Fork,
    Login,
    Logout,
    Name(Option<String>),
    SessionInfo,
    Copy,
    Settings,
    NotCommand,
}

pub trait LocalSlashCommandHost {
    fn slash_help(&mut self) -> Result<()>;
    fn slash_exit(&mut self) -> Result<()>;
    fn slash_new_session(&mut self) -> Result<()>;
    fn slash_compact(&mut self, instructions: Option<&str>) -> Result<()>;
    fn slash_model_select(&mut self, search: Option<&str>) -> Result<()>;
    fn slash_resume(&mut self) -> Result<()>;
    fn slash_tree(&mut self) -> Result<()>;
    fn slash_fork(&mut self) -> Result<()>;
    fn slash_login(&mut self) -> Result<()>;
    fn slash_logout(&mut self) -> Result<()>;
    fn slash_name(&mut self, name: Option<&str>) -> Result<()>;
    fn slash_session_info(&mut self) -> Result<()>;
    fn slash_copy(&mut self) -> Result<()>;
    fn slash_settings(&mut self) -> Result<()>;
}

pub fn dispatch_local_slash_command<H: LocalSlashCommandHost>(host: &mut H, text: &str) -> Result<bool> {
    match handle_slash_command(text) {
        SlashResult::NotCommand => return Ok(false),
        SlashResult::Help => host.slash_help()?,
        SlashResult::Exit => host.slash_exit()?,
        SlashResult::NewSession => host.slash_new_session()?,
        SlashResult::Compact(instructions) => host.slash_compact(instructions.as_deref())?,
        SlashResult::ModelSelect(search) => host.slash_model_select(search.as_deref())?,
        SlashResult::Resume => host.slash_resume()?,
        SlashResult::Tree => host.slash_tree()?,
        SlashResult::Fork => host.slash_fork()?,
        SlashResult::Login => host.slash_login()?,
        SlashResult::Logout => host.slash_logout()?,
        SlashResult::Name(name) => host.slash_name(name.as_deref())?,
        SlashResult::SessionInfo => host.slash_session_info()?,
        SlashResult::Copy => host.slash_copy()?,
        SlashResult::Settings => host.slash_settings()?,
    }
    Ok(true)
}

/// Get help text as lines (for display in TUI).
pub fn help_lines() -> Vec<String> {
    shared_slash_command_help_lines()
}

#[cfg(test)]
mod tests {
    use super::{dispatch_local_slash_command, handle_slash_command, LocalSlashCommandHost, SlashResult};

    #[derive(Default)]
    struct MockHost {
        calls: Vec<String>,
    }

    impl LocalSlashCommandHost for MockHost {
        fn slash_help(&mut self) -> anyhow::Result<()> { self.calls.push("help".into()); Ok(()) }
        fn slash_exit(&mut self) -> anyhow::Result<()> { self.calls.push("exit".into()); Ok(()) }
        fn slash_new_session(&mut self) -> anyhow::Result<()> { self.calls.push("new".into()); Ok(()) }
        fn slash_compact(&mut self, instructions: Option<&str>) -> anyhow::Result<()> {
            self.calls.push(format!("compact:{:?}", instructions));
            Ok(())
        }
        fn slash_model_select(&mut self, search: Option<&str>) -> anyhow::Result<()> {
            self.calls.push(format!("model:{:?}", search));
            Ok(())
        }
        fn slash_resume(&mut self) -> anyhow::Result<()> { self.calls.push("resume".into()); Ok(()) }
        fn slash_tree(&mut self) -> anyhow::Result<()> { self.calls.push("tree".into()); Ok(()) }
        fn slash_fork(&mut self) -> anyhow::Result<()> { self.calls.push("fork".into()); Ok(()) }
        fn slash_login(&mut self) -> anyhow::Result<()> { self.calls.push("login".into()); Ok(()) }
        fn slash_logout(&mut self) -> anyhow::Result<()> { self.calls.push("logout".into()); Ok(()) }
        fn slash_name(&mut self, name: Option<&str>) -> anyhow::Result<()> {
            self.calls.push(format!("name:{:?}", name));
            Ok(())
        }
        fn slash_session_info(&mut self) -> anyhow::Result<()> { self.calls.push("session".into()); Ok(()) }
        fn slash_copy(&mut self) -> anyhow::Result<()> { self.calls.push("copy".into()); Ok(()) }
        fn slash_settings(&mut self) -> anyhow::Result<()> { self.calls.push("settings".into()); Ok(()) }
    }

    #[test]
    fn parses_copy_command() {
        assert!(matches!(handle_slash_command("/copy"), SlashResult::Copy));
    }

    #[test]
    fn parses_name_and_settings_commands() {
        assert!(matches!(handle_slash_command("/settings"), SlashResult::Settings));
        assert_eq!(handle_slash_command("/name demo"), SlashResult::Name(Some("demo".into())));
    }

    #[test]
    fn dispatches_shared_local_command_through_host() {
        let mut host = MockHost::default();
        assert!(dispatch_local_slash_command(&mut host, "/model claude").unwrap());
        assert_eq!(host.calls, vec!["model:Some(\"claude\")".to_string()]);
    }
}
