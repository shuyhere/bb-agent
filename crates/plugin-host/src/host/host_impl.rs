use std::sync::atomic::AtomicU64;
use tokio::io::BufReader;
use tokio::process::Child;

use super::types::{RegisteredCommand, RegisteredTool, SharedUiHandler};

/// A running plugin host process that loads and executes TS plugins.
pub struct PluginHost {
    pub(super) child: Child,
    pub(super) stdin: tokio::process::ChildStdin,
    pub(super) stdout_reader: BufReader<tokio::process::ChildStdout>,
    pub(super) next_id: AtomicU64,
    pub(super) registered_tools: Vec<RegisteredTool>,
    pub(super) registered_commands: Vec<RegisteredCommand>,
    pub(super) plugin_count: usize,
    pub(super) ui_handler: Option<SharedUiHandler>,
}

impl PluginHost {
    /// Get the list of tools registered by plugins.
    pub fn registered_tools(&self) -> &[RegisteredTool] {
        &self.registered_tools
    }

    /// Get the list of commands registered by plugins.
    pub fn registered_commands(&self) -> &[RegisteredCommand] {
        &self.registered_commands
    }

    /// Get the number of loaded plugins.
    pub fn plugin_count(&self) -> usize {
        self.plugin_count
    }

    /// Set a UI handler for processing extension UI requests.
    pub fn set_ui_handler(&mut self, handler: SharedUiHandler) {
        self.ui_handler = Some(handler);
    }

    /// Remove the UI handler.
    pub fn clear_ui_handler(&mut self) {
        self.ui_handler = None;
    }
}
