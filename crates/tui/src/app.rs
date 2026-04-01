use bb_core::types::AgentMessage;
use std::io::Write;

use crate::chat;
use crate::editor::Editor;
use crate::status;

/// The main TUI application.
pub struct App {
    model_name: Option<String>,
}

impl App {
    pub fn new() -> Self {
        Self { model_name: None }
    }

    pub fn set_model(&mut self, name: &str) {
        self.model_name = Some(name.to_string());
    }

    /// Display a message in the chat view.
    pub fn display_message(&self, msg: &AgentMessage) {
        let lines = chat::render_message(msg);
        for line in lines {
            println!("{line}");
        }
    }

    /// Display all messages in a session context.
    pub fn display_messages(&self, messages: &[AgentMessage]) {
        for msg in messages {
            self.display_message(msg);
        }
    }

    /// Display the status bar.
    pub fn display_status(&self, tokens: Option<u64>, context_window: Option<u64>) {
        let line = status::render_status(
            self.model_name.as_deref(),
            tokens,
            context_window,
        );
        if !line.is_empty() {
            println!("{line}");
        }
    }

    /// Read user input. Returns None on exit (Ctrl+C/D).
    pub fn read_input(&self) -> Option<String> {
        let mut editor = Editor::new("> ");
        editor.read_line()
    }

    /// Print the welcome banner.
    pub fn print_banner(&self) {
        println!("bb-agent v{}", env!("CARGO_PKG_VERSION"));
        println!("Type your prompt, or Ctrl+C to exit.\n");
    }

    /// Print a separator line.
    pub fn separator(&self) {
        println!();
    }
}
