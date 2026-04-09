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
        "/paste-image" => SlashResult::PasteImage,
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
    PasteImage,
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
    fn slash_paste_image(&mut self) -> Result<()>;
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
        SlashResult::PasteImage => host.slash_paste_image()?,
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
        fn slash_paste_image(&mut self) -> anyhow::Result<()> {
            self.calls.push("paste-image".into());
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
    fn dispatches_shared_local_command_through_host() {
        let mut host = MockHost::default();
        assert!(dispatch_local_slash_command(&mut host, "/model claude").unwrap());
        assert_eq!(host.calls, vec!["model:Some(\"claude\")".to_string()]);
    }

    #[test]
    fn parses_and_dispatches_paste_image_command() {
        assert_eq!(
            handle_slash_command("/paste-image"),
            SlashResult::PasteImage
        );

        let mut host = MockHost::default();
        assert!(dispatch_local_slash_command(&mut host, "/paste-image").unwrap());
        assert_eq!(host.calls, vec!["paste-image".to_string()]);
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
}
