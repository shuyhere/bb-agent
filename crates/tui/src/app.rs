//! Legacy app wrapper — kept for backward compatibility.
//! The new TUI uses tui_core::TUI directly.

use bb_core::types::AgentMessage;

use crate::chat;
use crate::status;

/// The main TUI application (legacy wrapper).
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

    /// Print the welcome banner.
    pub fn print_banner(&self) {
        println!("bb-agent v{}", env!("CARGO_PKG_VERSION"));
        println!("Type your prompt, or Ctrl+C to exit.\n");
    }

    /// Print a separator line.
    pub fn separator(&self) {
        println!();
    }

    /// Read user input (simple stdin readline, not the TUI editor).
    /// Returns None on EOF/error.
    pub fn read_input(&self) -> Option<String> {
        use std::io::BufRead;
        print!("> ");
        std::io::Write::flush(&mut std::io::stdout()).ok();
        let stdin = std::io::stdin();
        let mut line = String::new();
        match stdin.lock().read_line(&mut line) {
            Ok(0) => None, // EOF
            Ok(_) => {
                let trimmed = line.trim().to_string();
                if trimmed.is_empty() { None } else { Some(trimmed) }
            }
            Err(_) => None,
        }
    }
}
