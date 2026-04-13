#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BashSafetyDisposition {
    Safe,
    ApprovalRequired,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BashSafetyAssessment {
    pub disposition: BashSafetyDisposition,
    pub title: String,
    pub reason: String,
}

pub fn classify_bash_command(command: &str) -> BashSafetyAssessment {
    let active_lines = command
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect::<Vec<_>>();

    if active_lines.is_empty() {
        return safe("Allow empty command");
    }

    if active_lines.len() > 1 {
        return approval_required(
            "Approval required for multi-step bash command",
            "Command contains multiple active shell lines",
        );
    }

    let line = active_lines[0];
    if contains_shell_control_operators(line) {
        return approval_required(
            "Approval required for shell control operators",
            "Command uses shell control operators, redirection, or substitution",
        );
    }

    for segment in line.split('|').map(str::trim) {
        if segment.is_empty() {
            return approval_required(
                "Approval required for complex bash pipeline",
                "Command contains an empty or malformed pipeline segment",
            );
        }

        let tokens = segment.split_whitespace().collect::<Vec<_>>();
        if tokens.is_empty() {
            continue;
        }

        let command_index = tokens
            .iter()
            .position(|token| !looks_like_env_assignment(token))
            .unwrap_or(0);
        let command_name = tokens[command_index];
        let args = &tokens[command_index + 1..];

        if !is_safe_command(command_name, args) {
            let detail = if command_name == "git" && !args.is_empty() {
                format!("Command uses git {} which may change state", args[0])
            } else {
                format!("Command `{command_name}` is not in the read-only allowlist")
            };
            return approval_required("Approval required for non-read-only bash command", &detail);
        }
    }

    safe("Allow read-only bash command")
}

fn safe(reason: &str) -> BashSafetyAssessment {
    BashSafetyAssessment {
        disposition: BashSafetyDisposition::Safe,
        title: "Read-only bash command".to_string(),
        reason: reason.to_string(),
    }
}

fn approval_required(title: &str, reason: &str) -> BashSafetyAssessment {
    BashSafetyAssessment {
        disposition: BashSafetyDisposition::ApprovalRequired,
        title: title.to_string(),
        reason: reason.to_string(),
    }
}

fn contains_shell_control_operators(line: &str) -> bool {
    line.contains("&&")
        || line.contains("||")
        || line.contains(';')
        || line.contains('&')
        || line.contains(">|")
        || line.contains(">>")
        || line.contains('>')
        || line.contains("<<")
        || line.contains("<<<")
        || line.contains('<')
        || line.contains("$(")
        || line.contains('`')
        || line.contains("|&")
}

fn looks_like_env_assignment(token: &str) -> bool {
    let Some((name, value)) = token.split_once('=') else {
        return false;
    };
    !name.is_empty()
        && !value.is_empty()
        && name
            .chars()
            .all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn is_safe_command(command_name: &str, args: &[&str]) -> bool {
    match command_name {
        "pwd" | "echo" | "printf" | "true" | "false" | "test" | "[" | "pwdx" | "which" | "type"
        | "realpath" | "basename" | "dirname" | "readlink" | "stat" | "file" | "wc" | "head"
        | "tail" | "cut" | "sort" | "uniq" | "grep" | "rg" | "ls" | "du" | "df" | "find"
        | "cat" | "sed" | "awk" | "env" | "printenv" | "id" | "whoami" | "uname" | "date"
        | "ps" => safe_command_flags_allowed(command_name, args),
        "git" => is_safe_git_subcommand(args),
        _ => false,
    }
}

fn safe_command_flags_allowed(command_name: &str, args: &[&str]) -> bool {
    match command_name {
        "sed" => !args
            .iter()
            .any(|arg| *arg == "-i" || *arg == "--in-place" || arg.starts_with("--in-place=")),
        "find" => !args.iter().any(|arg| {
            matches!(
                *arg,
                "-delete"
                    | "-exec"
                    | "-execdir"
                    | "-ok"
                    | "-okdir"
                    | "-fprint"
                    | "-fprint0"
                    | "-fprintf"
                    | "-fls"
            )
        }),
        _ => true,
    }
}

fn is_safe_git_subcommand(args: &[&str]) -> bool {
    let Some(subcommand) = args.first().copied() else {
        return false;
    };
    matches!(
        subcommand,
        "status"
            | "diff"
            | "show"
            | "log"
            | "branch"
            | "rev-parse"
            | "ls-files"
            | "grep"
            | "remote"
            | "config"
    )
}

#[cfg(test)]
mod tests {
    use super::{BashSafetyDisposition, classify_bash_command};

    #[test]
    fn allows_simple_read_only_commands() {
        let assessment = classify_bash_command("rg Tui crates/tui/src | head -n 5");
        assert_eq!(assessment.disposition, BashSafetyDisposition::Safe);
    }

    #[test]
    fn requires_approval_for_unknown_commands() {
        let assessment = classify_bash_command("cargo check --workspace");
        assert_eq!(
            assessment.disposition,
            BashSafetyDisposition::ApprovalRequired
        );
        assert!(assessment.reason.contains("read-only allowlist"));
    }

    #[test]
    fn requires_approval_for_mutating_safe_command_flags() {
        let assessment = classify_bash_command("sed -i 's/old/new/' Cargo.toml");
        assert_eq!(
            assessment.disposition,
            BashSafetyDisposition::ApprovalRequired
        );
    }

    #[test]
    fn requires_approval_for_shell_redirection() {
        let assessment = classify_bash_command("echo hi > /tmp/out.txt");
        assert_eq!(
            assessment.disposition,
            BashSafetyDisposition::ApprovalRequired
        );
    }

    #[test]
    fn allows_safe_git_queries_only() {
        let safe = classify_bash_command("git status --short");
        assert_eq!(safe.disposition, BashSafetyDisposition::Safe);

        let unsafe_git = classify_bash_command("git checkout -b feature");
        assert_eq!(
            unsafe_git.disposition,
            BashSafetyDisposition::ApprovalRequired
        );
    }
}
