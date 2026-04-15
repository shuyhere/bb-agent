use bb_tools::{Tool, builtin_tools};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ToolSelectionPreference {
    UseSettings,
    None,
    Only(Vec<String>),
}

impl Default for ToolSelectionPreference {
    fn default() -> Self {
        Self::UseSettings
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ToolSelection {
    All,
    None,
    Only(Vec<String>),
}

impl ToolSelectionPreference {
    pub(crate) fn resolve(&self, settings_tools: Option<&[String]>) -> ToolSelection {
        match self {
            Self::UseSettings => match settings_tools {
                Some(names) => ToolSelection::Only(names.to_vec()),
                None => ToolSelection::All,
            },
            Self::None => ToolSelection::None,
            Self::Only(names) => ToolSelection::Only(names.clone()),
        }
    }
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ToolSourceKind {
    Builtin,
    Extension,
}

#[cfg(test)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ToolDescriptor {
    pub name: String,
    pub description: String,
    pub source: ToolSourceKind,
}

struct RegisteredTool {
    #[cfg(test)]
    source: ToolSourceKind,
    tool: Box<dyn Tool>,
}

pub(crate) struct ToolRegistry {
    active_tools: Vec<Box<dyn Tool>>,
    tool_defs: Vec<serde_json::Value>,
    #[cfg(test)]
    active_names: Vec<String>,
    #[cfg(test)]
    available_tools: Vec<ToolDescriptor>,
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self {
            active_tools: Vec::new(),
            tool_defs: Vec::new(),
            #[cfg(test)]
            active_names: Vec::new(),
            #[cfg(test)]
            available_tools: Vec::new(),
        }
    }
}

impl ToolRegistry {
    #[cfg(test)]
    pub(crate) fn from_tools(tools: Vec<Box<dyn Tool>>) -> Self {
        Self::from_sources(Vec::new(), tools, ToolSelection::All)
    }

    pub(crate) fn from_sources(
        builtin: Vec<Box<dyn Tool>>,
        extensions: Vec<Box<dyn Tool>>,
        selection: ToolSelection,
    ) -> Self {
        let mut registered = Vec::new();
        registered.extend(builtin.into_iter().map(|tool| RegisteredTool {
            #[cfg(test)]
            source: ToolSourceKind::Builtin,
            tool,
        }));
        registered.extend(extensions.into_iter().map(|tool| RegisteredTool {
            #[cfg(test)]
            source: ToolSourceKind::Extension,
            tool,
        }));

        let deduped = dedupe_last_wins(registered);
        #[cfg(test)]
        let available_tools = deduped
            .iter()
            .map(|registered| ToolDescriptor {
                name: registered.tool.name().to_string(),
                description: registered.tool.description().to_string(),
                source: registered.source,
            })
            .collect::<Vec<_>>();

        #[cfg(test)]
        let (active_tools, active_names) = activate_tools(deduped, &selection);
        #[cfg(not(test))]
        let active_tools = activate_tools(deduped, &selection);
        let tool_defs = build_tool_defs(&active_tools);

        Self {
            active_tools,
            tool_defs,
            #[cfg(test)]
            active_names,
            #[cfg(test)]
            available_tools,
        }
    }

    pub(crate) fn from_builtin_and_extensions(
        extensions: Vec<Box<dyn Tool>>,
        selection: ToolSelection,
    ) -> Self {
        Self::from_sources(builtin_tools(), extensions, selection)
    }

    pub(crate) fn active_tools(&self) -> &[Box<dyn Tool>] {
        &self.active_tools
    }

    pub(crate) fn tool_defs(&self) -> &[serde_json::Value] {
        &self.tool_defs
    }

    #[cfg(test)]
    pub(crate) fn active_names(&self) -> &[String] {
        &self.active_names
    }

    #[cfg(test)]
    pub(crate) fn available_tools(&self) -> &[ToolDescriptor] {
        &self.available_tools
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.active_tools.len()
    }
}

fn dedupe_last_wins(tools: Vec<RegisteredTool>) -> Vec<RegisteredTool> {
    let mut last_index_by_name = HashMap::new();
    for (index, registered) in tools.iter().enumerate() {
        last_index_by_name.insert(registered.tool.name().to_string(), index);
    }

    tools
        .into_iter()
        .enumerate()
        .filter_map(|(index, registered)| {
            let is_last = last_index_by_name.get(registered.tool.name()).copied() == Some(index);
            is_last.then_some(registered)
        })
        .collect()
}

#[cfg(test)]
fn activate_tools(
    deduped: Vec<RegisteredTool>,
    selection: &ToolSelection,
) -> (Vec<Box<dyn Tool>>, Vec<String>) {
    match selection {
        ToolSelection::All => {
            let active_names = deduped
                .iter()
                .map(|registered| registered.tool.name().to_string())
                .collect();
            let active_tools = deduped
                .into_iter()
                .map(|registered| registered.tool)
                .collect();
            (active_tools, active_names)
        }
        ToolSelection::None => (Vec::new(), Vec::new()),
        ToolSelection::Only(requested_names) => {
            let mut requested = Vec::new();
            let mut seen = HashSet::new();
            for name in requested_names {
                if seen.insert(name.clone()) {
                    requested.push(name.clone());
                }
            }

            let mut by_name = HashMap::new();
            for registered in deduped {
                by_name.insert(registered.tool.name().to_string(), registered.tool);
            }

            let mut active_tools = Vec::new();
            let mut active_names = Vec::new();
            for name in requested {
                if let Some(tool) = by_name.remove(&name) {
                    active_names.push(name);
                    active_tools.push(tool);
                }
            }
            (active_tools, active_names)
        }
    }
}

#[cfg(not(test))]
fn activate_tools(deduped: Vec<RegisteredTool>, selection: &ToolSelection) -> Vec<Box<dyn Tool>> {
    match selection {
        ToolSelection::All => deduped
            .into_iter()
            .map(|registered| registered.tool)
            .collect(),
        ToolSelection::None => Vec::new(),
        ToolSelection::Only(requested_names) => {
            let mut requested = Vec::new();
            let mut seen = HashSet::new();
            for name in requested_names {
                if seen.insert(name.clone()) {
                    requested.push(name.clone());
                }
            }

            let mut by_name = HashMap::new();
            for registered in deduped {
                by_name.insert(registered.tool.name().to_string(), registered.tool);
            }

            let mut active_tools = Vec::new();
            for name in requested {
                if let Some(tool) = by_name.remove(&name) {
                    active_tools.push(tool);
                }
            }
            active_tools
        }
    }
}

pub(crate) fn build_tool_defs(tools: &[Box<dyn Tool>]) -> Vec<serde_json::Value> {
    tools
        .iter()
        .map(|tool| {
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": tool.name(),
                    "description": tool.description(),
                    "parameters": tool.parameters_schema(),
                }
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use serde_json::{Value, json};
    use tokio_util::sync::CancellationToken;

    struct NamedTool {
        name: &'static str,
        description: &'static str,
    }

    #[async_trait]
    impl Tool for NamedTool {
        fn name(&self) -> &str {
            self.name
        }

        fn description(&self) -> &str {
            self.description
        }

        fn parameters_schema(&self) -> Value {
            json!({"type": "object"})
        }

        async fn execute(
            &self,
            _params: Value,
            _ctx: &bb_tools::ToolContext,
            _cancel: CancellationToken,
        ) -> bb_core::error::BbResult<bb_tools::ToolResult> {
            unimplemented!("tool execution is not needed for registry tests")
        }
    }

    #[test]
    fn settings_tool_selection_defaults_to_all_when_unset() {
        let selection = ToolSelectionPreference::UseSettings.resolve(None);
        assert_eq!(selection, ToolSelection::All);
    }

    #[test]
    fn settings_tool_selection_uses_settings_when_present() {
        let selection = ToolSelectionPreference::UseSettings
            .resolve(Some(&["read".to_string(), "bash".to_string()]));
        assert_eq!(
            selection,
            ToolSelection::Only(vec!["read".to_string(), "bash".to_string()])
        );
    }

    #[test]
    fn extension_tool_overrides_builtin_with_same_name() {
        let registry = ToolRegistry::from_sources(
            vec![Box::new(NamedTool {
                name: "read",
                description: "builtin read",
            })],
            vec![Box::new(NamedTool {
                name: "read",
                description: "extension read",
            })],
            ToolSelection::All,
        );

        assert_eq!(registry.active_names(), &["read".to_string()]);
        assert_eq!(registry.available_tools().len(), 1);
        assert_eq!(
            registry.available_tools()[0].source,
            ToolSourceKind::Extension
        );
        assert_eq!(registry.available_tools()[0].description, "extension read");
    }

    #[test]
    fn explicit_selection_preserves_requested_order_and_ignores_unknown_names() {
        let registry = ToolRegistry::from_sources(
            vec![
                Box::new(NamedTool {
                    name: "read",
                    description: "read",
                }),
                Box::new(NamedTool {
                    name: "bash",
                    description: "bash",
                }),
            ],
            vec![Box::new(NamedTool {
                name: "my_tool",
                description: "custom",
            })],
            ToolSelection::Only(vec![
                "my_tool".to_string(),
                "bash".to_string(),
                "missing".to_string(),
                "bash".to_string(),
            ]),
        );

        assert_eq!(
            registry.active_names(),
            &["my_tool".to_string(), "bash".to_string()]
        );
        assert_eq!(registry.len(), 2);
    }
}
