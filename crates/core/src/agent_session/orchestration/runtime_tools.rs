use super::super::config::{PromptSource, StreamingBehavior};
use super::super::events::AgentSessionEvent;
use super::super::messages::{content_from_text_and_images, ImageContent, UserMessage};
use super::super::runtime::RuntimeBuildOptions;
use super::super::session::AgentSession;
use crate::tool_names::default_builtin_tool_names;

impl AgentSession {
    pub(crate) fn install_agent_subscription(&mut self) {
        let listeners = self.state.event_listeners.clone();

        self.emit_ref(&AgentSessionEvent::SessionStarted {
            event: self.state.session_start_event,
        });

        self.state.unsubscribe_agent = Some(Box::new(move || {
            let locked = listeners
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            for listener in locked.iter() {
                listener(&AgentSessionEvent::SessionShutdown);
            }
        }));
    }

    pub(crate) fn build_runtime(&mut self, options: RuntimeBuildOptions) {
        use super::super::runtime::{
            AgentTool as RtAgentTool, ToolDefinition as RtToolDefinition,
            ToolDefinitionEntry as RtToolDefinitionEntry, ToolPromptGuideline, ToolPromptSnippet,
        };

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
                default_builtin_tool_names()
                    .into_iter()
                    .map(|name| RtToolDefinition {
                        name,
                        description: None,
                    })
                    .collect()
            };
        self.state.base_tool_definitions = base_tools;

        let mut registry: Vec<RtAgentTool> = Vec::new();
        let mut definitions: Vec<RtToolDefinitionEntry> = Vec::new();
        let mut snippets: Vec<ToolPromptSnippet> = Vec::new();
        let mut guidelines: Vec<ToolPromptGuideline> = Vec::new();

        for def in &self.state.base_tool_definitions {
            registry.push(RtAgentTool {
                name: def.name.clone(),
            });
            definitions.push(RtToolDefinitionEntry {
                name: def.name.clone(),
                definition: def.clone(),
            });
        }

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

        if let Some(ref active_names) = options.active_tool_names {
            registry.retain(|tool| active_names.contains(&tool.name));
        }

        self.state.tool_registry = registry;
        self.state.tool_definitions = definitions;
        self.state.tool_prompt_snippets = snippets;
        self.state.tool_prompt_guidelines = guidelines;
    }

    pub(crate) fn emit_ref(&self, event: &AgentSessionEvent) {
        let listeners = self
            .state
            .event_listeners
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for listener in listeners.iter() {
            listener(event);
        }
    }

    pub(crate) fn emit_queue_update(&self) {
        self.emit_ref(&AgentSessionEvent::QueueUpdate {
            steering: self.state.steering_messages.clone(),
            follow_up: self.state.follow_up_messages.clone(),
        });
    }

    pub(crate) fn queue_steer(&mut self, text: String, images: Vec<ImageContent>) {
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

    pub(crate) fn queue_follow_up(&mut self, text: String, images: Vec<ImageContent>) {
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

    pub(crate) fn flush_pending_bash_messages(&mut self) {
        if self.state.pending_bash_messages.is_empty() {
            return;
        }
        self.state.pending_bash_messages.clear();
        self.emit_ref(&AgentSessionEvent::BashMessagesFlushed);
    }

    pub(crate) fn wait_for_retry(&mut self) {
        if self.state.retry_in_flight {
            self.state.retry_in_flight = false;
        }
    }
}
