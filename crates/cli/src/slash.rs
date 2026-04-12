use anyhow::Result;
use bb_tui::slash_commands::{install_help_lines, shared_slash_command_help_lines};

/// Handle shared local slash commands.
pub fn handle_slash_command(text: &str) -> SlashResult {
    let text = text.trim();
    match text {
        "/help" => SlashResult::Help,
        "/quit" | "/exit" => SlashResult::Exit,
        "/new" => SlashResult::NewSession,
        "/compact" => SlashResult::Compact(None),
        cmd if cmd.starts_with("/compact ") => {
            let Some(instructions) = cmd.strip_prefix("/compact ") else {
                return SlashResult::NotCommand;
            };
            SlashResult::Compact(Some(instructions.trim().to_string()))
        }
        "/model" => SlashResult::ModelSelect(None),
        cmd if cmd.starts_with("/model ") => {
            let Some(search) = cmd.strip_prefix("/model ") else {
                return SlashResult::NotCommand;
            };
            SlashResult::ModelSelect(Some(search.trim().to_string()))
        }
        "/resume" => SlashResult::Resume,
        "/tree" => SlashResult::Tree,
        "/fork" => SlashResult::Fork,
        "/login" => SlashResult::Login,
        "/logout" => SlashResult::Logout,
        "/session" => SlashResult::SessionInfo,
        "/copy" => SlashResult::Copy,
        "/settings" => SlashResult::Settings,
        "/hotkeys" => SlashResult::Hotkeys,
        "/reload" => SlashResult::Reload,
        "/name" => SlashResult::Name(None),
        cmd if cmd.starts_with("/name ") => {
            let Some(name) = cmd.strip_prefix("/name ") else {
                return SlashResult::NotCommand;
            };
            SlashResult::Name(Some(name.trim().to_string()))
        }
        "/export" => SlashResult::Export(None),
        cmd if cmd.starts_with("/export ") => {
            let Some(path) = cmd.strip_prefix("/export ") else {
                return SlashResult::NotCommand;
            };
            SlashResult::Export(Some(path.trim().to_string()))
        }
        "/import" => SlashResult::Import(None),
        cmd if cmd.starts_with("/import ") => {
            let Some(path) = cmd.strip_prefix("/import ") else {
                return SlashResult::NotCommand;
            };
            SlashResult::Import(Some(path.trim().to_string()))
        }
        cmd if cmd.starts_with("/image ") => {
            let Some(path) = cmd.strip_prefix("/image ") else {
                return SlashResult::NotCommand;
            };
            let path = path.trim();
            if path.is_empty() {
                SlashResult::NotCommand
            } else {
                SlashResult::Image(path.to_string())
            }
        }
        "/image" => SlashResult::NotCommand, // need a path argument
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
    Hotkeys,
    Reload,
    Export(Option<String>),
    Import(Option<String>),
    Image(String),
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
    fn slash_hotkeys(&mut self) -> Result<()>;
    fn slash_reload(&mut self) -> Result<()>;
    fn slash_export(&mut self, path: Option<&str>) -> Result<()>;
    fn slash_import(&mut self, path: Option<&str>) -> Result<()>;
    fn slash_image(&mut self, path: &str) -> Result<()>;
}

pub fn dispatch_local_slash_command<H: LocalSlashCommandHost>(
    host: &mut H,
    text: &str,
) -> Result<bool> {
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
        SlashResult::Hotkeys => host.slash_hotkeys()?,
        SlashResult::Reload => host.slash_reload()?,
        SlashResult::Export(path) => host.slash_export(path.as_deref())?,
        SlashResult::Import(path) => host.slash_import(path.as_deref())?,
        SlashResult::Image(path) => host.slash_image(&path)?,
    }
    Ok(true)
}

/// Get help text as lines (for display in TUI).
pub fn help_lines() -> Vec<String> {
    shared_slash_command_help_lines()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallCommand {
    pub local: bool,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallSlashAction {
    Help,
    Install(InstallCommand),
}

pub fn install_help_text() -> String {
    install_help_lines().join("\n")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillAdminAction {
    Help,
    List,
    Disable(String),
    Enable(String),
}

pub fn skill_help_text() -> String {
    [
        "/skill — manage which skills are loaded this session",
        "",
        "  /skill                  Show this help",
        "  /skill list             Show loaded skills and any disabled ones",
        "  /skill disable <name>   Disable a skill (source file is kept; it just won't load)",
        "  /skill enable  <name>   Re-enable a previously disabled skill",
        "",
        "The disabled list is persisted to global settings and applied on /reload.",
    ]
    .join("\n")
}

pub fn parse_skill_command(text: &str) -> Option<SkillAdminAction> {
    let text = text.trim();
    let rest = text.strip_prefix("/skill")?;
    // Make sure "/skillfoo" does NOT match "/skill".
    if !rest.is_empty() && !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let rest = rest.trim();
    if rest.is_empty() || rest == "--help" || rest == "-h" || rest == "help" {
        return Some(SkillAdminAction::Help);
    }
    if rest == "list" || rest == "ls" {
        return Some(SkillAdminAction::List);
    }
    if let Some(name) = rest.strip_prefix("disable ") {
        let name = name.trim();
        if name.is_empty() {
            return Some(SkillAdminAction::Help);
        }
        return Some(SkillAdminAction::Disable(name.to_string()));
    }
    if let Some(name) = rest.strip_prefix("enable ") {
        let name = name.trim();
        if name.is_empty() {
            return Some(SkillAdminAction::Help);
        }
        return Some(SkillAdminAction::Enable(name.to_string()));
    }
    if rest == "disable" || rest == "enable" {
        return Some(SkillAdminAction::Help);
    }
    Some(SkillAdminAction::Help)
}

pub fn parse_install_command(text: &str) -> Option<InstallSlashAction> {
    let text = text.trim();
    let rest = text.strip_prefix("/install")?.trim();
    if rest.is_empty() || rest == "--help" || rest == "-h" {
        return Some(InstallSlashAction::Help);
    }

    let (local, source) = if let Some(source) = rest.strip_prefix("-l ") {
        (true, source.trim())
    } else if let Some(source) = rest.strip_prefix("--local ") {
        (true, source.trim())
    } else {
        (false, rest)
    };

    if source.is_empty() || source == "--help" || source == "-h" {
        Some(InstallSlashAction::Help)
    } else {
        Some(InstallSlashAction::Install(InstallCommand {
            local,
            source: source.to_string(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        LocalSlashCommandHost, SlashResult, dispatch_local_slash_command, handle_slash_command,
        install_help_text, parse_install_command,
    };

    #[derive(Default)]
    struct MockHost {
        calls: Vec<String>,
    }

    impl LocalSlashCommandHost for MockHost {
        fn slash_help(&mut self) -> anyhow::Result<()> {
            self.calls.push("help".into());
            Ok(())
        }
        fn slash_exit(&mut self) -> anyhow::Result<()> {
            self.calls.push("exit".into());
            Ok(())
        }
        fn slash_new_session(&mut self) -> anyhow::Result<()> {
            self.calls.push("new".into());
            Ok(())
        }
        fn slash_compact(&mut self, instructions: Option<&str>) -> anyhow::Result<()> {
            self.calls.push(format!("compact:{:?}", instructions));
            Ok(())
        }
        fn slash_model_select(&mut self, search: Option<&str>) -> anyhow::Result<()> {
            self.calls.push(format!("model:{:?}", search));
            Ok(())
        }
        fn slash_resume(&mut self) -> anyhow::Result<()> {
            self.calls.push("resume".into());
            Ok(())
        }
        fn slash_tree(&mut self) -> anyhow::Result<()> {
            self.calls.push("tree".into());
            Ok(())
        }
        fn slash_fork(&mut self) -> anyhow::Result<()> {
            self.calls.push("fork".into());
            Ok(())
        }
        fn slash_login(&mut self) -> anyhow::Result<()> {
            self.calls.push("login".into());
            Ok(())
        }
        fn slash_logout(&mut self) -> anyhow::Result<()> {
            self.calls.push("logout".into());
            Ok(())
        }
        fn slash_name(&mut self, name: Option<&str>) -> anyhow::Result<()> {
            self.calls.push(format!("name:{:?}", name));
            Ok(())
        }
        fn slash_session_info(&mut self) -> anyhow::Result<()> {
            self.calls.push("session".into());
            Ok(())
        }
        fn slash_copy(&mut self) -> anyhow::Result<()> {
            self.calls.push("copy".into());
            Ok(())
        }
        fn slash_settings(&mut self) -> anyhow::Result<()> {
            self.calls.push("settings".into());
            Ok(())
        }
        fn slash_hotkeys(&mut self) -> anyhow::Result<()> {
            self.calls.push("hotkeys".into());
            Ok(())
        }
        fn slash_reload(&mut self) -> anyhow::Result<()> {
            self.calls.push("reload".into());
            Ok(())
        }
        fn slash_export(&mut self, path: Option<&str>) -> anyhow::Result<()> {
            self.calls.push(format!("export:{:?}", path));
            Ok(())
        }
        fn slash_import(&mut self, path: Option<&str>) -> anyhow::Result<()> {
            self.calls.push(format!("import:{:?}", path));
            Ok(())
        }
        fn slash_image(&mut self, path: &str) -> anyhow::Result<()> {
            self.calls.push(format!("image:{}", path));
            Ok(())
        }
    }

    #[test]
    fn parses_copy_command() {
        assert!(matches!(handle_slash_command("/copy"), SlashResult::Copy));
    }

    #[test]
    fn parses_name_and_settings_commands() {
        assert!(matches!(
            handle_slash_command("/settings"),
            SlashResult::Settings
        ));
        assert_eq!(
            handle_slash_command("/name demo"),
            SlashResult::Name(Some("demo".into()))
        );
    }

    #[test]
    fn does_not_treat_mid_message_slash_text_as_command() {
        assert!(matches!(
            handle_slash_command("please do not run /compact in the middle"),
            SlashResult::NotCommand
        ));
        assert!(matches!(
            handle_slash_command("prefix /model sonnet suffix"),
            SlashResult::NotCommand
        ));
    }

    #[test]
    fn dispatches_shared_local_command_through_host() {
        let mut host = MockHost::default();
        assert!(dispatch_local_slash_command(&mut host, "/model claude").unwrap());
        assert_eq!(host.calls, vec!["model:Some(\"claude\")".to_string()]);
    }

    #[test]
    fn parses_install_command_with_optional_local_flag() {
        assert_eq!(
            parse_install_command("/install npm:demo"),
            Some(super::InstallSlashAction::Install(super::InstallCommand {
                local: false,
                source: "npm:demo".to_string(),
            }))
        );
        assert_eq!(
            parse_install_command("/install -l git:github.com/demo/repo"),
            Some(super::InstallSlashAction::Install(super::InstallCommand {
                local: true,
                source: "git:github.com/demo/repo".to_string(),
            }))
        );
        assert_eq!(
            parse_install_command("/install"),
            Some(super::InstallSlashAction::Help)
        );
        assert_eq!(
            parse_install_command("/install --help"),
            Some(super::InstallSlashAction::Help)
        );
        assert!(install_help_text().contains("/install [-l|--local] <source>"));
    }

    #[test]
    fn parses_skill_admin_commands() {
        use super::{SkillAdminAction, parse_skill_command, skill_help_text};
        assert_eq!(parse_skill_command("/skill"), Some(SkillAdminAction::Help));
        assert_eq!(
            parse_skill_command("/skill --help"),
            Some(SkillAdminAction::Help)
        );
        assert_eq!(
            parse_skill_command("/skill list"),
            Some(SkillAdminAction::List)
        );
        assert_eq!(
            parse_skill_command("/skill ls"),
            Some(SkillAdminAction::List)
        );
        assert_eq!(
            parse_skill_command("/skill disable shape"),
            Some(SkillAdminAction::Disable("shape".to_string()))
        );
        assert_eq!(
            parse_skill_command("/skill enable  my skill  "),
            Some(SkillAdminAction::Enable("my skill".to_string()))
        );
        // Bare disable/enable falls back to help.
        assert_eq!(
            parse_skill_command("/skill disable"),
            Some(SkillAdminAction::Help)
        );
        // /skillfoo should NOT match /skill.
        assert_eq!(parse_skill_command("/skillfoo"), None);
        // Unrelated slash commands are not touched.
        assert_eq!(parse_skill_command("/install"), None);
        assert!(skill_help_text().contains("/skill disable <name>"));
    }
}
