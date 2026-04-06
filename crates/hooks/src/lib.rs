//! Hook event types and the shared event bus used by BB-Agent extensions.

pub mod bus;
mod error;
pub mod events;

pub use bus::{EventBus, SharedEventBus};
pub use error::{HookError, Result};
pub use events::{Event, HookResult, ToolCallEvent};
