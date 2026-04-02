use std::sync::atomic::AtomicU64;
use tokio::io::BufReader;
use tokio::process::Child;

use super::types::{RegisteredCommand, RegisteredTool};

/// A running plugin host process that loads and executes TS plugins.
pub struct PluginHost {
    pub(super) child: Child,
    pub(super) stdin: tokio::process::ChildStdin,
    pub(super) stdout_reader: BufReader<tokio::process::ChildStdout>,
    pub(super) next_id: AtomicU64,
    pub(super) registered_tools: Vec<RegisteredTool>,
    pub(super) registered_commands: Vec<RegisteredCommand>,
    pub(super) plugin_count: usize,
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
}
