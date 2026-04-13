//! BB-Agent TUI crate
//!
//! Provides the terminal user interface components including
//! the interactive TUI editor, chat view, and syntax highlighting.

pub mod app;
pub mod chat;
pub mod component;
pub mod components;
pub mod editor;
pub mod error;
pub mod footer;
pub mod footer_data;
pub mod fullscreen;
pub mod fuzzy;
pub mod kill_ring;
pub mod markdown;
pub mod model_selector;
pub mod renderer;
pub mod select_list;
pub mod session_selector;
pub mod slash_commands;
pub mod status;
pub mod syntax;
pub mod terminal;
pub mod theme;
pub mod tool_preview;
pub mod tree_selector;
pub(crate) mod ui_hints;
pub mod tui_core;
pub mod undo_stack;
pub mod utils;

pub use components::{BgFn, BorderColorFn, BoxComponent, DynamicBorder};
