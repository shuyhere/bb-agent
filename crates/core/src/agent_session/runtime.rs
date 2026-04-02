#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BashExecutionMessage {
    pub command: String,
    pub status: BashExecutionStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BashExecutionStatus {
    Pending,
    Running,
    Completed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolDefinition {
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentTool {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolDefinitionEntry {
    pub name: String,
    pub definition: ToolDefinition,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolPromptSnippet {
    pub tool_name: String,
    pub snippet: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolPromptGuideline {
    pub tool_name: String,
    pub guidelines: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeBuildOptions {
    pub active_tool_names: Option<Vec<String>>,
    pub include_all_extension_tools: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeHandle {
    pub kind: &'static str,
}

impl RuntimeHandle {
    pub fn placeholder(kind: &'static str) -> Self {
        Self { kind }
    }
}
