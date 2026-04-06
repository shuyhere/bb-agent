use super::super::error::AgentSessionError;
use super::super::events::AgentSessionEvent;
use super::super::session::AgentSession;

impl AgentSession {
    pub(crate) fn try_execute_extension_command(&self, text: &str) -> bool {
        let Some((command_name, args)) = parse_slash_command(text) else {
            return false;
        };

        let found = self
            .state
            .resource_bootstrap
            .extensions
            .registered_commands
            .iter()
            .any(|cmd| cmd.invocation_name == command_name);

        if !found {
            return false;
        }

        self.emit_ref(&AgentSessionEvent::ExtensionCommandExecuted {
            command: command_name.to_owned(),
            args: args.map(str::to_owned),
        });

        true
    }

    pub(crate) fn expand_skill_command(&self, text: String) -> String {
        let Some((skill_name, user_args)) = parse_skill_command(&text) else {
            return text;
        };

        self.state
            .resource_bootstrap
            .skills
            .iter()
            .find(|skill| skill.info.name == skill_name)
            .map(|skill| format_resource_content(&skill.content, user_args))
            .unwrap_or(text)
    }

    pub(crate) fn expand_prompt_template(&self, text: String) -> String {
        let Some((command_name, user_args)) = parse_slash_command(&text) else {
            return text;
        };

        self.state
            .resource_bootstrap
            .prompts
            .iter()
            .find(|prompt| prompt.info.slash_command_name() == command_name)
            .map(|prompt| format_resource_content(&prompt.content, user_args))
            .unwrap_or(text)
    }

    pub(crate) fn throw_if_extension_command(&self, text: &str) -> Result<(), AgentSessionError> {
        if self.is_registered_extension_command(text) {
            return Err(AgentSessionError::ExtensionCommandCannotBeQueued);
        }
        Ok(())
    }

    fn is_registered_extension_command(&self, text: &str) -> bool {
        let Some((command_name, _)) = parse_slash_command(text) else {
            return false;
        };

        self.state
            .resource_bootstrap
            .extensions
            .registered_commands
            .iter()
            .any(|command| command.invocation_name == command_name)
    }
}

fn parse_skill_command(text: &str) -> Option<(&str, Option<&str>)> {
    let trimmed = text.trim();
    let remainder = trimmed.strip_prefix("/skill:")?;
    split_command_name_and_args(remainder)
}

fn parse_slash_command(text: &str) -> Option<(&str, Option<&str>)> {
    let trimmed = text.trim();
    let remainder = trimmed.strip_prefix('/')?;
    split_command_name_and_args(remainder)
}

fn split_command_name_and_args(input: &str) -> Option<(&str, Option<&str>)> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }

    match trimmed.find(char::is_whitespace) {
        Some(index) => {
            let name = trimmed[..index].trim();
            if name.is_empty() {
                return None;
            }
            let args = trimmed[index..].trim();
            Some((name, (!args.is_empty()).then_some(args)))
        }
        None => Some((trimmed, None)),
    }
}

fn format_resource_content(content: &str, user_args: Option<&str>) -> String {
    match user_args {
        Some(args) => format!("{}\n\nUser: {}", content.trim_end(), args),
        None => content.to_string(),
    }
}
