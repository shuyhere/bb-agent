use super::*;

impl AgentSessionExtensions {
    pub fn bind_extensions(&mut self, bindings: ExtensionBindings) {
        if let Some(ui_context) = bindings.ui_context {
            self.extension_ui_context = Some(ui_context);
        }
        if let Some(actions) = bindings.command_context_actions {
            self.extension_command_context_actions = actions;
        }
        if let Some(shutdown_handler) = bindings.shutdown_handler {
            self.extension_shutdown_handler = Some(shutdown_handler);
        }
        if let Some(on_error) = bindings.on_error {
            self.extension_error_listener = Some(on_error);
        }

        if self.extension_runner.is_some() {
            self.apply_extension_bindings();
            let event = self.session_start_event;
            if let Some(runner) = self.extension_runner.as_mut() {
                runner.emit_session_start(event);
            }
            self.extend_resources_from_extensions(event.reason);
        }
    }

    pub fn extend_resources_from_extensions(&mut self, reason: SessionStartReason) {
        let Some(runner) = self.extension_runner.as_ref() else {
            return;
        };
        if !runner.has_handlers("resources_discover") {
            return;
        }

        let discovered = runner.emit_resources_discover(&self.cwd, reason);
        if discovered.skill_paths.is_empty()
            && discovered.prompt_paths.is_empty()
            && discovered.theme_paths.is_empty()
        {
            return;
        }

        let extension_paths = ResourceExtensionPaths {
            skill_paths: self.build_extension_resource_paths(&discovered.skill_paths),
            prompt_paths: self.build_extension_resource_paths(&discovered.prompt_paths),
            theme_paths: self.build_extension_resource_paths(&discovered.theme_paths),
        };

        self.resource_loader.extend_resources(extension_paths);
        self.base_system_prompt = self.rebuild_system_prompt(&self.get_active_tool_names());
        self.system_prompt = self.base_system_prompt.clone();
    }

    pub fn build_extension_resource_paths(
        &self,
        entries: &[DiscoveredResourcePath],
    ) -> Vec<ResourcePathEntry> {
        entries
            .iter()
            .map(|entry| {
                let source = self.get_extension_source_label(&entry.extension_path);
                let base_dir = if entry.extension_path.starts_with('<') {
                    None
                } else {
                    Path::new(&entry.extension_path)
                        .parent()
                        .map(Path::to_path_buf)
                };
                ResourcePathEntry {
                    path: entry.path.clone(),
                    metadata: ResourcePathMetadata {
                        source,
                        scope: ResourceScope::Temporary,
                        origin: ResourceOrigin::TopLevel,
                        base_dir,
                    },
                }
            })
            .collect()
    }

    pub fn get_extension_source_label(&self, extension_path: &str) -> String {
        if extension_path.starts_with('<') {
            return format!("extension:{}", extension_path.trim_matches(&['<', '>'][..]));
        }

        let stem = Path::new(extension_path)
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or(extension_path);
        format!("extension:{stem}")
    }

    pub fn apply_extension_bindings(&mut self) {
        if let Some(runner) = self.extension_runner.as_mut() {
            runner.set_ui_context(self.extension_ui_context.clone());
            runner.bind_command_context(self.extension_command_context_actions.clone());
            runner.on_error(self.extension_error_listener.clone());
        }
    }

    pub fn refresh_current_model_from_registry(&mut self) {
        let Some(current_model) = self.model.clone() else {
            return;
        };

        let refreshed = self
            .model_registry
            .find(&current_model.provider, &current_model.id);
        if let Some(refreshed_model) = refreshed
            && refreshed_model != current_model
        {
            self.model = Some(refreshed_model);
        }
    }

    pub fn bind_extension_core(&self) -> ExtensionCoreBindings {
        ExtensionCoreBindings {
            commands: self.get_commands(),
            active_tools: self.get_active_tool_names(),
            all_tools: self.get_all_tools(),
            model: self.model.clone(),
            system_prompt: self.system_prompt.clone(),
        }
    }
}
