//! Shared fullscreen transcript surface.
//!
//! Keep the public API intentionally small so CLI entry code stays a thin
//! adapter and future controls/streaming/runtime work lands on this shared
//! stack instead of growing a second fullscreen implementation.

mod frame;
mod input;
mod layout;
mod menus;
mod navigation;
mod projection;
mod renderer;
mod runtime;
mod scheduler;
mod search;
mod terminal;
mod tool_format;
mod transcript;
mod types;
mod viewport;

#[cfg(test)]
mod tests;

pub use runtime::{run, run_with_channels};
pub use types::{
    FullscreenAppConfig, FullscreenCommand, FullscreenFooterData, FullscreenNoteLevel,
    FullscreenOutcome, FullscreenSubmission,
};
pub use transcript::{BlockId, BlockKind, NewBlock, Transcript, TranscriptBlock, TranscriptError};
