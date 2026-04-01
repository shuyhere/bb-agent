pub mod host;
pub mod discovery;
pub mod protocol;

pub use host::{PluginHost, PluginHostError, RegisteredTool, RegisteredCommand};
pub use discovery::{PluginInfo, PluginScope, discover_plugins};
