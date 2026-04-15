use async_trait::async_trait;
use bb_core::error::BbResult;
use bb_provider::{Provider, registry::Model};
use serde_json::Value;
use std::{collections::HashMap, future::Future, path::PathBuf, pin::Pin, sync::Arc};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolScheduling {
    ReadOnly,
    MutatingPaths(Vec<PathBuf>),
    MutatingUnknown,
}

impl ToolScheduling {
    pub fn single_mutating_path(path: PathBuf) -> Self {
        Self::MutatingPaths(vec![path])
    }
}

/// Result from a tool execution.
#[derive(Clone, Debug)]
pub struct ToolResult {
    pub content: Vec<bb_core::types::ContentBlock>,
    pub details: Option<Value>,
    pub is_error: bool,
    pub artifact_path: Option<PathBuf>,
}

pub type OnOutputFn = Box<dyn Fn(&str) + Send + Sync>;
pub type ToolApprovalFuture = Pin<Box<dyn Future<Output = ToolApprovalOutcome> + Send>>;
pub type RequestToolApprovalFn =
    Arc<dyn Fn(ToolApprovalRequest) -> ToolApprovalFuture + Send + Sync>;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ToolExecutionMode {
    #[default]
    Interactive,
    NonInteractive,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolApprovalRequest {
    pub tool_name: String,
    pub title: String,
    pub command: String,
    pub reason: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolApprovalDecision {
    ApprovedOnce,
    ApprovedForSession,
    Denied,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ToolApprovalOutcome {
    pub decision: ToolApprovalDecision,
}

impl ToolApprovalOutcome {
    pub const fn approved(&self) -> bool {
        matches!(
            self.decision,
            ToolApprovalDecision::ApprovedOnce | ToolApprovalDecision::ApprovedForSession
        )
    }

    pub const fn approved_for_session(&self) -> bool {
        matches!(self.decision, ToolApprovalDecision::ApprovedForSession)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ExecutionPolicy {
    #[default]
    Safety,
    Yolo,
}

impl ExecutionPolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Safety => "safety",
            Self::Yolo => "yolo",
        }
    }

    pub fn restricts_workspace_writes(self) -> bool {
        matches!(self, Self::Safety)
    }

    pub fn write_scope_label(self) -> &'static str {
        match self {
            Self::Safety => "current project only",
            Self::Yolo => "full access",
        }
    }
}

impl From<bb_core::settings::ExecutionMode> for ExecutionPolicy {
    fn from(value: bb_core::settings::ExecutionMode) -> Self {
        match value {
            bb_core::settings::ExecutionMode::Safety => Self::Safety,
            bb_core::settings::ExecutionMode::Yolo => Self::Yolo,
        }
    }
}

#[derive(Clone)]
pub struct WebSearchRuntime {
    pub provider: Arc<dyn Provider>,
    pub model: Model,
    pub api_key: String,
    pub base_url: String,
    pub headers: HashMap<String, String>,
    pub enabled: bool,
}

/// Context available to tools during execution.
pub struct ToolContext {
    pub cwd: PathBuf,
    pub artifacts_dir: PathBuf,
    pub execution_policy: ExecutionPolicy,
    pub on_output: Option<OnOutputFn>,
    pub web_search: Option<WebSearchRuntime>,
    pub execution_mode: ToolExecutionMode,
    pub request_approval: Option<RequestToolApprovalFn>,
}

/// Trait for built-in and custom tools.
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters_schema(&self) -> Value;

    /// Classify whether this call is read-only or may mutate files.
    ///
    /// Mutating tools should override this to return either concrete file paths
    /// for per-file serialization or `MutatingUnknown` when the touched files
    /// cannot be determined up front.
    fn scheduling(&self, _params: &Value, _ctx: &ToolContext) -> ToolScheduling {
        ToolScheduling::ReadOnly
    }

    async fn execute(
        &self,
        params: Value,
        ctx: &ToolContext,
        cancel: CancellationToken,
    ) -> BbResult<ToolResult>;
}
