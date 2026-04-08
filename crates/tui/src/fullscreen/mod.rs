//! Shared fullscreen transcript surface.
//!
//! Keep the public API intentionally small so CLI entry code stays a thin
//! adapter and future controls/streaming/runtime work lands on this shared
//! stack instead of growing a second fullscreen implementation.

mod events;
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
pub mod spinner;
mod streaming;
mod terminal;
mod tool_format;
mod transcript;
mod types;
pub mod vibewords;
mod viewport;

pub use events::{try_read_clipboard_image, try_read_clipboard_text};

#[cfg(test)]
mod tests;

pub use runtime::{run, run_with_channels};
pub use tool_format::{
    format_tool_call_content, format_tool_call_title, format_tool_result_content,
};
pub use transcript::{BlockId, BlockKind, NewBlock, Transcript, TranscriptBlock, TranscriptError};
pub use types::{
    FullscreenAppConfig, FullscreenApprovalChoice, FullscreenApprovalDialog, FullscreenAuthDialog,
    FullscreenAuthStep, FullscreenAuthStepState, FullscreenCommand, FullscreenFooterData,
    FullscreenNoteLevel, FullscreenOutcome, FullscreenSubmission, HistoricalToolState,
};
