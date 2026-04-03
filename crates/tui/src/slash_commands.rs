use crate::select_list::SelectItem;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlashCommandSpec {
    pub command: &'static str,
    pub menu_detail: &'static str,
    pub help_usage: &'static str,
    pub help_detail: &'static str,
}

const SHARED_SLASH_COMMANDS: &[SlashCommandSpec] = &[
    SlashCommandSpec {
        command: "/help",
        menu_detail: "Show help",
        help_usage: "/help",
        help_detail: "Show this help",
    },
    SlashCommandSpec {
        command: "/new",
        menu_detail: "Start a new session",
        help_usage: "/new",
        help_detail: "Start a new session",
    },
    SlashCommandSpec {
        command: "/resume",
        menu_detail: "Resume a previous session",
        help_usage: "/resume",
        help_detail: "Resume a previous session",
    },
    SlashCommandSpec {
        command: "/model",
        menu_detail: "Switch model",
        help_usage: "/model [name]",
        help_detail: "Switch model",
    },
    SlashCommandSpec {
        command: "/compact",
        menu_detail: "Compact conversation context",
        help_usage: "/compact",
        help_detail: "Compact conversation context",
    },
    SlashCommandSpec {
        command: "/copy",
        menu_detail: "Copy last response to clipboard",
        help_usage: "/copy",
        help_detail: "Copy last response to clipboard",
    },
    SlashCommandSpec {
        command: "/tree",
        menu_detail: "Navigate session tree",
        help_usage: "/tree",
        help_detail: "Navigate session tree",
    },
    SlashCommandSpec {
        command: "/fork",
        menu_detail: "Fork current session",
        help_usage: "/fork",
        help_detail: "Fork current session",
    },
    SlashCommandSpec {
        command: "/name",
        menu_detail: "Set session display name",
        help_usage: "/name <name>",
        help_detail: "Set session display name",
    },
    SlashCommandSpec {
        command: "/session",
        menu_detail: "Show current session info",
        help_usage: "/session",
        help_detail: "Show current session info",
    },
    SlashCommandSpec {
        command: "/login",
        menu_detail: "Login to a provider",
        help_usage: "/login",
        help_detail: "Login to a provider",
    },
    SlashCommandSpec {
        command: "/logout",
        menu_detail: "Logout from a provider",
        help_usage: "/logout",
        help_detail: "Logout from a provider",
    },
    SlashCommandSpec {
        command: "/settings",
        menu_detail: "Show settings info",
        help_usage: "/settings",
        help_detail: "Show settings info",
    },
    SlashCommandSpec {
        command: "/quit",
        menu_detail: "Exit",
        help_usage: "/quit",
        help_detail: "Exit",
    },
];

pub fn shared_slash_commands() -> &'static [SlashCommandSpec] {
    SHARED_SLASH_COMMANDS
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
    use super::{shared_slash_command_help_lines, shared_slash_command_select_items};

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
}
