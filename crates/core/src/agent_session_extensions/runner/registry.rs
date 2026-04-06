use super::*;

impl AgentSessionExtensions {
    pub fn refresh_tool_registry(&mut self, options: RefreshToolRegistryOptions) {
        let previous_registry_names: BTreeSet<String> =
            self.tool_registry.keys().cloned().collect();
        let previous_active_tool_names = self.get_active_tool_names();

        let mut all_custom_tools = self
            .extension_runner
            .as_ref()
            .map(|runner| runner.get_all_registered_tools())
            .unwrap_or_default();
        all_custom_tools.extend(self.custom_tools.clone());

        let mut definition_registry: BTreeMap<String, ToolDefinitionEntry> = self
            .base_tool_definitions
            .iter()
            .map(|(name, definition)| {
                (
                    name.clone(),
                    ToolDefinitionEntry {
                        definition: definition.clone(),
                        source_info: SourceInfo {
                            path: format!("<builtin:{name}>"),
                            source: "builtin".to_owned(),
                        },
                    },
                )
            })
            .collect();

        for tool in &all_custom_tools {
            definition_registry.insert(
                tool.definition.name.clone(),
                ToolDefinitionEntry {
                    definition: tool.definition.clone(),
                    source_info: tool.source_info.clone(),
                },
            );
        }

        self.tool_definitions = definition_registry.clone();
        self.tool_prompt_snippets = definition_registry
            .values()
            .filter_map(|entry| {
                self.normalize_prompt_snippet(entry.definition.prompt_snippet.clone())
                    .map(|snippet| (entry.definition.name.clone(), snippet))
            })
            .collect();
        self.tool_prompt_guidelines = definition_registry
            .values()
            .filter_map(|entry| {
                let guidelines =
                    self.normalize_prompt_guidelines(entry.definition.prompt_guidelines.clone());
                (!guidelines.is_empty()).then(|| (entry.definition.name.clone(), guidelines))
            })
            .collect();

        let mut tool_registry: BTreeMap<String, AgentTool> = self
            .base_tool_definitions
            .values()
            .map(|definition| {
                (
                    definition.name.clone(),
                    AgentTool {
                        name: definition.name.clone(),
                    },
                )
            })
            .collect();

        let wrapped_extension_tools: Vec<AgentTool> = all_custom_tools
            .iter()
            .map(|tool| AgentTool {
                name: tool.definition.name.clone(),
            })
            .collect();
        for tool in &wrapped_extension_tools {
            tool_registry.insert(tool.name.clone(), tool.clone());
        }
        self.tool_registry = tool_registry;

        let mut next_active_tool_names = options
            .active_tool_names
            .unwrap_or_else(|| previous_active_tool_names.clone());

        if options.include_all_extension_tools {
            next_active_tool_names
                .extend(wrapped_extension_tools.iter().map(|tool| tool.name.clone()));
        } else if next_active_tool_names == previous_active_tool_names {
            for tool_name in self.tool_registry.keys() {
                if !previous_registry_names.contains(tool_name) {
                    next_active_tool_names.push(tool_name.clone());
                }
            }
        }

        self.set_active_tools_by_name(unique_preserving_order(next_active_tool_names));
    }

    pub fn build_runtime(&mut self, options: RuntimeBuildOptions) {
        let base_tool_definitions = if let Some(overrides) = &self.base_tools_override {
            overrides
                .values()
                .map(|tool| {
                    (
                        tool.name.clone(),
                        ToolDefinition {
                            name: tool.name.clone(),
                            prompt_snippet: None,
                            prompt_guidelines: Vec::new(),
                        },
                    )
                })
                .collect()
        } else {
            create_all_tool_definitions()
        };
        self.base_tool_definitions = base_tool_definitions;

        let mut extensions_result = self.resource_loader.get_extensions();
        for (name, value) in options.flag_values {
            extensions_result.runtime.flag_values.insert(name, value);
        }

        let has_extensions = !extensions_result.extensions.is_empty();
        let has_custom_tools = !self.custom_tools.is_empty();
        self.extension_runner = if has_extensions || has_custom_tools {
            Some(ExtensionRunnerState::new(
                extensions_result.extensions,
                extensions_result.runtime,
                self.cwd.clone(),
            ))
        } else {
            None
        };
        if self.extension_runner.is_some() {
            let _ = self.bind_extension_core();
            self.apply_extension_bindings();
        }

        let default_active_tool_names = if let Some(overrides) = &self.base_tools_override {
            overrides.keys().cloned().collect()
        } else {
            vec![
                "read".to_owned(),
                "bash".to_owned(),
                "edit".to_owned(),
                "write".to_owned(),
            ]
        };
        let base_active_tool_names = options
            .active_tool_names
            .unwrap_or(default_active_tool_names);
        self.refresh_tool_registry(RefreshToolRegistryOptions {
            active_tool_names: Some(base_active_tool_names),
            include_all_extension_tools: options.include_all_extension_tools,
        });
    }

    pub fn reload(&mut self) {
        let previous_flag_values = self
            .extension_runner
            .as_ref()
            .map(|runner| runner.get_flag_values())
            .unwrap_or_default();

        if let Some(runner) = self.extension_runner.as_mut() {
            runner.emit_session_shutdown();
        }
        self.settings.reload();
        self.resource_loader.reload();
        self.build_runtime(RuntimeBuildOptions {
            active_tool_names: Some(self.get_active_tool_names()),
            flag_values: previous_flag_values,
            include_all_extension_tools: true,
        });

        let has_bindings = self.extension_ui_context.is_some()
            || !self.extension_command_context_actions.is_empty()
            || self.extension_shutdown_handler.is_some()
            || self.extension_error_listener.is_some();
        if has_bindings {
            if let Some(runner) = self.extension_runner.as_mut() {
                runner.emit_session_start(SessionStartEvent {
                    reason: SessionStartReason::Reload,
                });
            }
            self.extend_resources_from_extensions(SessionStartReason::Reload);
        }
    }

    pub fn get_commands(&self) -> Vec<SlashCommandInfo> {
        let extension_commands = self
            .extension_runner
            .as_ref()
            .map(|runner| runner.get_registered_commands())
            .unwrap_or_default()
            .into_iter()
            .map(|command| SlashCommandInfo {
                name: command.invocation_name,
                description: command.description,
                source: SlashCommandSource::Extension,
                source_info: command.source_info,
            });

        let template_commands =
            self.prompt_templates
                .iter()
                .cloned()
                .map(|template| SlashCommandInfo {
                    name: template.name,
                    description: template.description,
                    source: SlashCommandSource::Prompt,
                    source_info: template.source_info,
                });

        let skill_commands = self
            .resource_loader
            .get_skills()
            .skills
            .into_iter()
            .map(|skill| SlashCommandInfo {
                name: format!("skill:{}", skill.name),
                description: skill.description,
                source: SlashCommandSource::Skill,
                source_info: skill.source_info,
            });

        extension_commands
            .chain(template_commands)
            .chain(skill_commands)
            .collect()
    }

    pub fn get_active_tool_names(&self) -> Vec<String> {
        self.active_tool_names.clone()
    }

    pub fn set_active_tools_by_name(&mut self, tool_names: Vec<String>) {
        self.active_tool_names = tool_names;
    }

    pub fn get_all_tools(&self) -> Vec<String> {
        self.tool_registry.keys().cloned().collect()
    }

    pub(super) fn rebuild_system_prompt(&self, active_tool_names: &[String]) -> String {
        let base_prompt = self
            .base_system_prompt
            .split("\n\nTool guidance:\n")
            .next()
            .unwrap_or(&self.base_system_prompt)
            .trim_end()
            .to_string();

        let mut sections = Vec::new();
        for tool_name in active_tool_names {
            if let Some(snippet) = self.tool_prompt_snippets.get(tool_name) {
                sections.push(format!("- {tool_name}: {snippet}"));
            }
            if let Some(guidelines) = self.tool_prompt_guidelines.get(tool_name) {
                for guideline in guidelines {
                    sections.push(format!("- {tool_name}: {guideline}"));
                }
            }
        }

        if sections.is_empty() {
            base_prompt
        } else {
            format!("{base_prompt}\n\nTool guidance:\n{}", sections.join("\n"))
        }
    }

    fn normalize_prompt_snippet(&self, snippet: Option<String>) -> Option<String> {
        snippet.and_then(|value| {
            let trimmed = value.trim().to_owned();
            (!trimmed.is_empty()).then_some(trimmed)
        })
    }

    fn normalize_prompt_guidelines(&self, guidelines: Vec<String>) -> Vec<String> {
        guidelines
            .into_iter()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
            .collect()
    }
}

pub fn create_all_tool_definitions() -> BTreeMap<String, ToolDefinition> {
    ["read", "bash", "edit", "write"]
        .into_iter()
        .map(|name| {
            (
                name.to_owned(),
                ToolDefinition {
                    name: name.to_owned(),
                    prompt_snippet: None,
                    prompt_guidelines: Vec::new(),
                },
            )
        })
        .collect()
}

fn unique_preserving_order(values: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut deduped = Vec::new();
    for value in values {
        if seen.insert(value.clone()) {
            deduped.push(value);
        }
    }
    deduped
}
