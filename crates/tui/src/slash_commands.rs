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
        command: "/help",
        menu_detail: "Show help",
        help_usage: "/help",
        help_detail: "Show this help",
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
        command: "/resume",
        menu_detail: "Resume a previous session",
        help_usage: "/resume",
        help_detail: "Resume a previous session",
        accepts_arguments: false,
    },
    SlashCommandSpec {
        command: "/model",
        menu_detail: "Switch model",
        help_usage: "/model [name]",
        help_detail: "Switch model",
        accepts_arguments: true,
    },
    SlashCommandSpec {
        command: "/compact",
        menu_detail: "Compact conversation context",
        help_usage: "/compact",
        help_detail: "Compact conversation context",
        accepts_arguments: true,
    },
    SlashCommandSpec {
        command: "/copy",
        menu_detail: "Copy last response to clipboard",
        help_usage: "/copy",
        help_detail: "Copy last response to clipboard",
        accepts_arguments: false,
    },
    SlashCommandSpec {
        command: "/tree",
        menu_detail: "Navigate session tree",
        help_usage: "/tree",
        help_detail: "Navigate session tree",
        accepts_arguments: false,
    },
    SlashCommandSpec {
        command: "/fork",
        menu_detail: "Fork current session",
        help_usage: "/fork",
        help_detail: "Fork current session",
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
        menu_detail: "Show current session info",
        help_usage: "/session",
        help_detail: "Show current session info",
        accepts_arguments: false,
    },
    SlashCommandSpec {
        command: "/login",
        menu_detail: "Login to a provider",
        help_usage: "/login",
        help_detail: "Login to a provider",
        accepts_arguments: false,
    },
    SlashCommandSpec {
        command: "/logout",
        menu_detail: "Logout from a provider",
        help_usage: "/logout",
        help_detail: "Logout from a provider",
        accepts_arguments: false,
    },
    SlashCommandSpec {
        command: "/settings",
        menu_detail: "Show settings info",
        help_usage: "/settings",
        help_detail: "Show settings info",
        accepts_arguments: false,
    },
    SlashCommandSpec {
        command: "/quit",
        menu_detail: "Exit",
        help_usage: "/quit",
        help_detail: "Exit",
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
    lines.push("  Shortcuts:".into());
    lines.push("    Ctrl+C         Abort / clear".into());
    lines.push("    Ctrl+D         Exit (empty editor)".into());
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
    }

    #[test]
    fn submission_match_handles_argument_forms() {
        assert!(matches_shared_local_slash_submission("/model claude"));
        assert!(matches_shared_local_slash_submission("/name demo"));
        assert!(!matches_shared_local_slash_submission("/help extra"));
    }
}
