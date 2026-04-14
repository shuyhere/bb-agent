//! Hook event types and the shared event bus used by BB-Agent extensions.
//!
//! # Example
//!
//! ```no_run
//! use std::sync::Arc;
//!
//! use bb_hooks::{Event, EventBus, HookResult, ToolCallEvent};
//!
//! # tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
//! let bus = EventBus::new();
//! bus.on(
//!     "tool_call",
//!     "safety-plugin",
//!     Arc::new(|event| {
//!         if let Event::ToolCall(tool_call) = event
//!             && tool_call.tool_name() == "bash"
//!         {
//!             return Some(HookResult {
//!                 reason: Some("reviewed".into()),
//!                 ..Default::default()
//!             });
//!         }
//!         None
//!     }),
//! )
//! .await;
//!
//! let result = bus
//!     .emit(&Event::ToolCall(ToolCallEvent::new(
//!         "call-1",
//!         "bash",
//!         serde_json::json!({ "command": "pwd" }),
//!     )))
//!     .await;
//! assert!(result.is_some());
//! # });
//! ```

pub mod bus;
mod error;
pub mod events;

pub use bus::{EventBus, SharedEventBus};
pub use error::{HookError, Result};
pub use events::{
    CompactPrep, ContextEvent, Event, HookResult, InputEvent, ToolCallEvent, ToolResultEvent,
    TreePrep,
};
