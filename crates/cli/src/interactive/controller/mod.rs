use super::components;
use super::events::{
    ChatItem, InteractiveMessage, InteractiveRenderState, PendingMessages,
    QueuedMessage as RenderQueuedMessage, QueuedMessageMode, ToolCallContent,
    assistant_message_from_parts,
};
use super::interactive_commands::SelectorKind;
use super::model_selector_overlay::{ModelSelectorOverlay, ModelSelectorOverlayAction};
use super::api::{InteractiveModeOptions, InteractiveResult, InteractiveSessionSetup};
use super::state::{
    InteractiveController, KeyAction, KeyBinding, QueuedMessage, QueuedMessageKind,
    SubmitAction, SubmitMatch, SubmitOutcome, SubmitRoute,
};
use super::status_loader::{StatusLoaderComponent, StatusLoaderStyle};
use bb_core::agent_loop::AgentLoopEvent;
use bb_core::agent_session::{ModelRef, PromptOptions, ThinkingLevel};
use bb_core::agent_session_runtime::{AgentSessionRuntimeHost, RuntimeModelRef};
use bb_provider::registry::{ApiType, Model, ModelRegistry};
use bb_session::{compaction, store};
use bb_tui::component::{Component, Container, Focusable, Spacer, Text};
use bb_tui::editor::Editor;
use bb_tui::footer::{Footer, FooterData, FooterDataProvider};
use bb_tui::model_selector::ModelSelector;
use bb_tui::terminal::{Terminal, TerminalEvent};
use bb_tui::tui_core::TUI;
use bb_tui::utils::word_wrap;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rusqlite::params;
use std::any::Any;
use std::collections::VecDeque;
use std::error::Error;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio::sync::mpsc::UnboundedReceiver;

mod agent_events;
mod command_actions;
mod editor_lifecycle;
mod interaction_controls;
mod key_actions;
mod mode;
mod model_actions;
mod rendering;
mod runtime;
mod shared;
mod submission_flow;
mod ui_state;

pub use mode::{InteractiveMode, run_interactive};

use mode::{UIContainers, StreamingState, QueueState, RenderCache, InteractionState};
use shared::{SharedContainer, SharedEditorWrapper};

