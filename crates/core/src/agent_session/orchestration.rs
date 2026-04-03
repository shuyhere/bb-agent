use super::config::{PromptSource, StreamingBehavior};
use super::error::AgentSessionError;
use super::events::AgentSessionEvent;
use super::messages::{ImageContent, UserMessage, content_from_text_and_images};
use super::runtime::RuntimeBuildOptions;
use super::session::AgentSession;

impl AgentSession {
    pub(super) fn install_agent_subscription(&mut self) {
        let listeners = self.state.event_listeners.clone();

        // Emit session start event at the session lifecycle boundary.
        self.emit_ref(&AgentSessionEvent::SessionStarted {
            event: self.state.session_start_event.clone(),
        });

        // The unsubscribe callback emits session shutdown when invoked.
        self.state.unsubscribe_agent = Some(Box::new(move || {
            let locked = listeners
                .lock()
                .expect("agent session event listener mutex poisoned");
            for listener in locked.iter() {
                listener(&AgentSessionEvent::SessionShutdown);
            }
        }));
    }

    pub(super) fn build_runtime(&mut self, options: RuntimeBuildOptions) {
        use super::runtime::{
            AgentTool as RtAgentTool, ToolDefinition as RtToolDefinition,
            ToolDefinitionEntry as RtToolDefinitionEntry, ToolPromptGuideline,
            ToolPromptSnippet,
        };

        // 1. Base tool definitions from overrides or built-in defaults.
        let base_tools: Vec<RtToolDefinition> =
            if let Some(ref overrides) = self.state.base_tools_override {
                overrides
                    .iter()
                    .map(|tool| RtToolDefinition {
                        name: tool.name.clone(),
                        description: None,
                    })
                    .collect()
            } else {
                ["read", "bash", "edit", "write"]
                    .iter()
                    .map(|&name| RtToolDefinition {
                        name: name.to_owned(),
                        description: None,
                    })
                    .collect()
            };
        self.state.base_tool_definitions = base_tools;

        // 2. Build the combined tool registry from base + custom + extension tools.
        let mut registry: Vec<RtAgentTool> = Vec::new();
        let mut definitions: Vec<RtToolDefinitionEntry> = Vec::new();
        let mut snippets: Vec<ToolPromptSnippet> = Vec::new();
        let mut guidelines: Vec<ToolPromptGuideline> = Vec::new();

        // -- base tools
        for def in &self.state.base_tool_definitions {
            registry.push(RtAgentTool {
                name: def.name.clone(),
            });
            definitions.push(RtToolDefinitionEntry {
                name: def.name.clone(),
                definition: def.clone(),
            });
        }

        // -- custom tools from config
        for custom_def in &self.state.custom_tools {
            if !registry.iter().any(|t| t.name == custom_def.name) {
                registry.push(RtAgentTool {
                    name: custom_def.name.clone(),
                });
                definitions.push(RtToolDefinitionEntry {
                    name: custom_def.name.clone(),
                    definition: custom_def.clone(),
                });
            }
        }

        // -- extension-registered tools
        if options.include_all_extension_tools {
            for reg_tool in &self.state.resource_bootstrap.extensions.registered_tools {
                let tool_name = &reg_tool.definition.name;
                if !registry.iter().any(|t| t.name == *tool_name) {
                    registry.push(RtAgentTool {
                        name: tool_name.clone(),
                    });
                    definitions.push(RtToolDefinitionEntry {
                        name: tool_name.clone(),
                        definition: RtToolDefinition {
                            name: tool_name.clone(),
                            description: None,
                        },
                    });
                    if let Some(snippet) = &reg_tool.definition.prompt_snippet {
                        let trimmed = snippet.trim();
                        if !trimmed.is_empty() {
                            snippets.push(ToolPromptSnippet {
                                tool_name: tool_name.clone(),
                                snippet: trimmed.to_owned(),
                            });
                        }
                    }
                    let tool_guidelines: Vec<String> = reg_tool
                        .definition
                        .prompt_guidelines
                        .iter()
                        .map(|g| g.trim().to_owned())
                        .filter(|g| !g.is_empty())
                        .collect();
                    if !tool_guidelines.is_empty() {
                        guidelines.push(ToolPromptGuideline {
                            tool_name: tool_name.clone(),
                            guidelines: tool_guidelines,
                        });
                    }
                }
            }
        }

        // 3. Filter by active tool names when specified.
        if let Some(ref active_names) = options.active_tool_names {
            registry.retain(|tool| active_names.contains(&tool.name));
        }

        self.state.tool_registry = registry;
        self.state.tool_definitions = definitions;
        self.state.tool_prompt_snippets = snippets;
        self.state.tool_prompt_guidelines = guidelines;
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

    pub(super) fn try_execute_extension_command(&self, text: &str) -> bool {
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

        // Extension commands are executed (not queued) when submitted via prompt.
        let result = session.prompt("/hello world", PromptOptions::default());
        assert_eq!(result, Ok(()));

        // Extension commands cannot be delivered as steer/follow-up messages.
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
