use super::types::FullscreenNoteLevel;
use std::time::{Duration, Instant};

use bb_core::types::ContentBlock;
use bb_session::{store::EntryRow, tree::TreeNode};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

use crate::select_list::SelectItem;

use super::{
    frame::build_frame,
    layout::Size,
    runtime::FullscreenState,
    scheduler::RenderScheduler,
    tool_format::{format_tool_call_content, format_tool_call_title, format_tool_result_content},
    transcript::{BlockId, BlockKind, NewBlock, Transcript},
    types::{
        FullscreenAppConfig, FullscreenApprovalChoice, FullscreenApprovalDialog, FullscreenCommand,
        FullscreenMode, FullscreenSubmission, HistoricalToolState,
    },
};

mod approval;
mod common;
mod frame_and_rendering;
mod interaction;
mod menus_and_commands;
