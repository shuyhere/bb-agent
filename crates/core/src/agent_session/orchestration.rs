use super::config::{PromptSource, StreamingBehavior};
use super::error::AgentSessionError;
use super::events::AgentSessionEvent;
use super::messages::{ImageContent, UserMessage, content_from_text_and_images};
use super::runtime::RuntimeBuildOptions;
use super::session::AgentSession;

impl AgentSession {
    pub(super) fn install_agent_subscription(&mut self) {
        // TODO: hook concrete runtime agent events.
        self.state.unsubscribe_agent = Some(Box::new(|| {}));
    }

    pub(super) fn build_runtime(&mut self, _options: RuntimeBuildOptions) {
        // TODO: port runtime tool / extension initialization.
    }

    pub(super) fn emit_ref(&self, event: &AgentSessionEvent) {
        let listeners = self
            .state
            .event_listeners
            .lock()
            .expect("agent session event listener mutex poisoned");
        for listener in listeners.iter() {
            listener(event);
        }
    }

    pub(super) fn emit_queue_update(&self) {
        self.emit_ref(&AgentSessionEvent::QueueUpdate {
            steering: self.state.steering_messages.clone(),
            follow_up: self.state.follow_up_messages.clone(),
        });
    }

    pub(super) fn try_execute_extension_command(&self, _text: &str) -> bool {
        // TODO: integrate extension command execution.
        false
    }

    pub(super) fn expand_skill_command(&self, text: String) -> String {
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

    pub(super) fn expand_prompt_template(&self, text: String) -> String {
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

    pub(super) fn queue_steer(&mut self, text: String, images: Vec<ImageContent>) {
        self.state.steering_messages.push(text.clone());
        self.emit_queue_update();
        self.emit_ref(&AgentSessionEvent::UserMessageQueued {
            delivery: StreamingBehavior::Steer,
            message: UserMessage {
                content: content_from_text_and_images(text, images),
                source: PromptSource::Extension,
            },
        });
    }

    pub(super) fn queue_follow_up(&mut self, text: String, images: Vec<ImageContent>) {
        self.state.follow_up_messages.push(text.clone());
        self.emit_queue_update();
        self.emit_ref(&AgentSessionEvent::UserMessageQueued {
            delivery: StreamingBehavior::FollowUp,
            message: UserMessage {
                content: content_from_text_and_images(text, images),
                source: PromptSource::Extension,
            },
        });
    }

    pub(super) fn throw_if_extension_command(&self, text: &str) -> Result<(), AgentSessionError> {
        if self.is_registered_extension_command(text) {
            return Err(AgentSessionError::ExtensionCommandCannotBeQueued);
        }
        Ok(())
    }

    pub(super) fn flush_pending_bash_messages(&mut self) {
        if self.state.pending_bash_messages.is_empty() {
            return;
        }
        self.state.pending_bash_messages.clear();
        self.emit_ref(&AgentSessionEvent::BashMessagesFlushed);
    }

    pub(super) fn wait_for_retry(&mut self) {
        if self.state.retry_in_flight {
            self.state.retry_in_flight = false;
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_session::{AgentSessionConfig, PromptOptions};
    use crate::agent_session_extensions::{
        ExtensionsResult, PromptTemplateDefinition, PromptTemplateInfo, RegisteredCommand,
        SessionResourceBootstrap, SkillDefinition, SkillInfo, SourceInfo,
    };

    fn test_session() -> AgentSession {
        AgentSession::new(AgentSessionConfig {
            resource_bootstrap: SessionResourceBootstrap {
                extensions: ExtensionsResult {
                    registered_commands: vec![RegisteredCommand {
                        invocation_name: "hello".to_string(),
                        description: "Say hello".to_string(),
                        source_info: SourceInfo {
                            path: "ext.js".to_string(),
                            source: "extension:test".to_string(),
                        },
                    }],
                    ..ExtensionsResult::default()
                },
                skills: vec![SkillDefinition {
                    info: SkillInfo {
                        name: "review".to_string(),
                        description: "Review skill".to_string(),
                        source_info: SourceInfo {
                            path: "skill.md".to_string(),
                            source: "settings:test".to_string(),
                        },
                    },
                    content: "# Review\nUse the review workflow".to_string(),
                }],
                prompts: vec![PromptTemplateDefinition {
                    info: PromptTemplateInfo {
                        name: "summarize".to_string(),
                        description: "Summarize content".to_string(),
                        source_info: SourceInfo {
                            path: "prompt.md".to_string(),
                            source: "settings:test".to_string(),
                        },
                    },
                    content: "Summarize the current state".to_string(),
                }],
            },
            ..AgentSessionConfig::default()
        })
    }

    #[test]
    fn expands_skill_command_with_user_args() {
        let session = test_session();
        let expanded = session.expand_skill_command("/skill:review focus on tests".to_string());
        assert_eq!(
            expanded,
            "# Review\nUse the review workflow\n\nUser: focus on tests"
        );
    }

    #[test]
    fn expands_prompt_template_with_user_args() {
        let session = test_session();
        let expanded = session.expand_prompt_template("/summarize pending changes".to_string());
        assert_eq!(
            expanded,
            "Summarize the current state\n\nUser: pending changes"
        );
    }

    #[test]
    fn unknown_commands_are_left_unchanged() {
        let session = test_session();
        assert_eq!(
            session.expand_skill_command("/skill:missing test".to_string()),
            "/skill:missing test"
        );
        assert_eq!(
            session.expand_prompt_template("/missing test".to_string()),
            "/missing test"
        );
    }

    #[test]
    fn registered_extension_commands_cannot_be_queued() {
        let mut session = test_session();
        session.state.model = Some(crate::agent_session::ModelRef {
            provider: "test".to_string(),
            id: "model".to_string(),
            reasoning: false,
        });

        let result = session.prompt("/hello world", PromptOptions::default());
        assert_eq!(
            result,
            Err(AgentSessionError::ExtensionCommandCannotBeQueued)
        );

        let steer_result = session.steer("/hello world", Vec::new());
        assert_eq!(
            steer_result,
            Err(AgentSessionError::ExtensionCommandCannotBeQueued)
        );
    }

    #[test]
    fn prompt_templates_are_not_treated_as_extension_commands() {
        let mut session = test_session();
        session.state.model = Some(crate::agent_session::ModelRef {
            provider: "test".to_string(),
            id: "model".to_string(),
            reasoning: false,
        });

        let result = session.prompt("/summarize release notes", PromptOptions::default());
        assert!(result.is_ok());
    }
}
