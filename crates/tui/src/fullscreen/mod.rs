//! Shared fullscreen transcript surface.
//!
//! Keep the public API intentionally small so CLI entry code stays a thin
//! adapter and future controls/streaming/runtime work lands on this shared
//! stack instead of growing a second fullscreen implementation.

mod frame;
mod layout;
mod projection;
mod renderer;
mod runtime;
mod scheduler;
mod terminal;
mod transcript;
mod viewport;

pub use runtime::{
    FullscreenAppConfig, FullscreenCommand, FullscreenNoteLevel, FullscreenOutcome, run,
    run_with_channels,
};
pub use transcript::{BlockId, BlockKind, NewBlock, Transcript, TranscriptBlock, TranscriptError};
