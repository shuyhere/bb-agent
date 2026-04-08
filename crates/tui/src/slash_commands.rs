use crate::select_list::SelectItem;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlashCommandSpec {
    pub command: &'static str,
    pub menu_detail: &'static str,
    pub help_usage: &'static str,
    pub help_detail: &'static str,
    pub accepts_arguments: bool,
}

const SHARED_SLASH_COMMANDS: &[SlashCommandSpec] = &[
    SlashCommandSpec {
        command: "/settings",
        menu_detail: "Open settings menu",
        help_usage: "/settings",
        help_detail: "Open settings menu",
        accepts_arguments: false,
    },
    SlashCommandSpec {
        command: "/model",
        menu_detail: "Select model",
        help_usage: "/model [name]",
        help_detail: "Select model (opens selector)",
        accepts_arguments: true,
    },
    SlashCommandSpec {
        command: "/export",
        menu_detail: "Export session (HTML/JSONL)",
        help_usage: "/export [path]",
        help_detail: "Export session (HTML default, or .html/.jsonl)",
        accepts_arguments: true,
    },
    SlashCommandSpec {
        command: "/import",
        menu_detail: "Import session from JSONL",
        help_usage: "/import <path>",
        help_detail: "Import and resume a session from a JSONL file",
        accepts_arguments: true,
    },
    SlashCommandSpec {
        command: "/copy",
        menu_detail: "Copy last response to clipboard",
        help_usage: "/copy",
        help_detail: "Copy last agent message to clipboard",
        accepts_arguments: false,
    },
    SlashCommandSpec {
        command: "/name",
        menu_detail: "Set session display name",
        help_usage: "/name <name>",
        help_detail: "Set session display name",
        accepts_arguments: true,
    },
    SlashCommandSpec {
        command: "/session",
        menu_detail: "Show session info and stats",
        help_usage: "/session",
        help_detail: "Show session info and stats",
        accepts_arguments: false,
    },
    SlashCommandSpec {
        command: "/hotkeys",
        menu_detail: "Show keyboard shortcuts",
        help_usage: "/hotkeys",
        help_detail: "Show all keyboard shortcuts",
        accepts_arguments: false,
    },
    SlashCommandSpec {
        command: "/fork",
        menu_detail: "Fork from a previous message",
        help_usage: "/fork",
        help_detail: "Create a new fork from a previous message",
        accepts_arguments: false,
    },
    SlashCommandSpec {
        command: "/tree",
        menu_detail: "Navigate session tree",
        help_usage: "/tree",
        help_detail: "Navigate session tree (switch branches)",
        accepts_arguments: false,
    },
    SlashCommandSpec {
        command: "/login",
        menu_detail: "Login with OAuth provider",
        help_usage: "/login",
        help_detail: "Login with OAuth provider",
        accepts_arguments: false,
    },
    SlashCommandSpec {
        command: "/logout",
        menu_detail: "Logout from OAuth provider",
        help_usage: "/logout",
        help_detail: "Logout from OAuth provider",
        accepts_arguments: false,
    },
    SlashCommandSpec {
        command: "/new",
        menu_detail: "Start a new session",
        help_usage: "/new",
        help_detail: "Start a new session",
        accepts_arguments: false,
    },
    SlashCommandSpec {
        command: "/compact",
        menu_detail: "Compact the session context",
        help_usage: "/compact [instructions]",
        help_detail: "Manually compact the session context",
        accepts_arguments: true,
    },
    SlashCommandSpec {
        command: "/resume",
        menu_detail: "Resume a different session",
        help_usage: "/resume",
        help_detail: "Resume a different session",
        accepts_arguments: false,
    },
    SlashCommandSpec {
        command: "/reload",
        menu_detail: "Reload extensions, skills, and prompts",
        help_usage: "/reload",
        help_detail: "Reload keybindings, extensions, skills, prompts",
        accepts_arguments: false,
    },
    SlashCommandSpec {
        command: "/install",
        menu_detail: "Install package source: npm:pkg, git:repo, path, or URL",
        help_usage: "/install [-l|--local] <source>",
        help_detail: "Install a package source and auto-reload skills/extensions/prompts. Sources can be npm:pkg, git:repo/url, a local path, or an archive URL. Use -l/--local for project-only install.",
        accepts_arguments: true,
    },
    SlashCommandSpec {
        command: "/update",
        menu_detail: "Check for a newer BB-Agent version",
        help_usage: "/update",
        help_detail: "Check for updates now",
        accepts_arguments: false,
    },
    SlashCommandSpec {
        command: "/image",
        menu_detail: "Attach image to next prompt",
        help_usage: "/image <path>",
        help_detail: "Attach an image file to the next message sent to the model",
        accepts_arguments: true,
    },
    SlashCommandSpec {
        command: "/paste-image",
        menu_detail: "Attach image from clipboard",
        help_usage: "/paste-image",
        help_detail: "Read an image from the system clipboard and attach it to the next prompt",
        accepts_arguments: false,
    },
    SlashCommandSpec {
        command: "/help",
        menu_detail: "Show help",
        help_usage: "/help",
        help_detail: "Show this help",
        accepts_arguments: false,
    },
    SlashCommandSpec {
        command: "/quit",
        menu_detail: "Quit",
        help_usage: "/quit",
        help_detail: "Quit bb-agent",
        accepts_arguments: false,
    },
];

pub fn shared_slash_commands() -> &'static [SlashCommandSpec] {
    SHARED_SLASH_COMMANDS
}

pub fn matches_shared_local_slash_submission(text: &str) -> bool {
    let text = text.trim();
    if !text.starts_with('/') {
        return false;
    }

    let Some(command) = text.split_whitespace().next() else {
        return false;
    };

    shared_slash_commands()
        .iter()
        .find(|spec| spec.command == command)
        .map(|spec| text == command || spec.accepts_arguments)
        .unwrap_or(false)
}

pub fn shared_slash_command_select_items() -> Vec<SelectItem> {
    shared_slash_commands()
        .iter()
        .map(|spec| SelectItem {
            label: spec.command.to_string(),
            detail: Some(spec.menu_detail.to_string()),
            value: spec.command.to_string(),
        })
        .collect()
}

pub fn install_help_lines() -> Vec<String> {
    vec![
        "Usage:".into(),
        "  /install [-l|--local] <source>".into(),
        String::new(),
        "Description:".into(),
        "  Install a package source and auto-reload skills, extensions, and prompts.".into(),
        "  Use /install --help to show this guide again.".into(),
        String::new(),
        "Supported source forms:".into(),
        "  npm:<package>                       Install from npm".into(),
        "  git:<repo-or-url>                  Install from git".into(),
        "  ./path or /absolute/path           Install from a local directory".into(),
        "  https://...                        Install from a remote archive/repo URL".into(),
        String::new(),
        "Options:".into(),
        "  -l, --local                        Install into the detected project root only".into(),
        String::new(),
        "Examples:".into(),
        "  /install npm:bb-example-skill".into(),
        "  /install -l npm:my-project-skill".into(),
        "  /install -l ./my-skill".into(),
        "  /install git:https://github.com/org/repo.git".into(),
        "  /install https://example.com/package.tar.gz".into(),
    ]
}

pub fn shared_slash_command_help_lines() -> Vec<String> {
    let usage_width = shared_slash_commands()
        .iter()
        .map(|spec| spec.help_usage.len())
        .max()
        .unwrap_or(0);

    let mut lines = vec!["  Available commands:".into()];
    lines.extend(shared_slash_commands().iter().map(|spec| {
        format!(
            "    {:<width$}  {}",
            spec.help_usage,
            spec.help_detail,
            width = usage_width
        )
    }));
    lines.push(String::new());
    lines.push("  Install examples:".into());
    lines.push("    /install npm:bb-example-skill".into());
    lines.push("    /install -l ./my-skill".into());
    lines.push("    /install git:https://github.com/org/repo.git".into());
    lines.push("    /install https://example.com/package.tar.gz".into());
    lines.push("    /install --help".into());
    lines.push(String::new());
    lines.push("  Shortcuts:".into());
    lines.push("    Ctrl+C         Abort / clear".into());
    lines.push("    Ctrl+D         Exit (empty editor)".into());
    lines.push("    Ctrl+V         Paste clipboard text or attach clipboard image".into());
    lines.push("    /copy          Copy last agent message to clipboard".into());
    lines.push("    /paste-image   Read clipboard image and attach it to the next prompt".into());
    lines.push("    !command       Run bash directly".into());
    lines
}

#[cfg(test)]
mod tests {
    use super::{
        matches_shared_local_slash_submission, shared_slash_command_help_lines,
        shared_slash_command_select_items,
    };

    #[test]
    fn shared_registry_contains_copy_command() {
        let commands = shared_slash_command_select_items();
        assert!(commands.iter().any(|item| item.value == "/copy"));
    }

    #[test]
    fn help_lines_include_argument_forms() {
        let help = shared_slash_command_help_lines().join("\n");
        assert!(help.contains("/model [name]"));
        assert!(help.contains("/name <name>"));
        assert!(help.contains("/install [-l|--local] <source>"));
        assert!(help.contains("npm:bb-example-skill"));
        assert!(help.contains("/update"));
    }

    #[test]
    fn submission_match_handles_argument_forms() {
        assert!(matches_shared_local_slash_submission("/model claude"));
        assert!(matches_shared_local_slash_submission("/name demo"));
        assert!(matches_shared_local_slash_submission("/install npm:demo"));
        assert!(matches_shared_local_slash_submission("/update"));
        assert!(!matches_shared_local_slash_submission("/help extra"));
    }
}
