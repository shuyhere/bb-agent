pub mod discovery;
pub mod error;
pub mod host;
pub mod protocol;

pub use discovery::{PluginInfo, PluginScope, discover_plugins};
pub use host::{
    DefaultUiHandler, PluginContext, PluginHost, PluginHostError, RegisteredCommand,
    RegisteredTool, SharedUiHandler, UiHandler, UiRequest, UiResponse, default_ui_response,
};
