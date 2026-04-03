use bb_tui::fullscreen::{BlockKind, FullscreenAppConfig, NewBlock, Transcript};

use crate::interactive::InteractiveEntryOptions;

pub(super) struct InteractiveFullscreenState {
    config: FullscreenAppConfig,
}

impl InteractiveFullscreenState {
    pub(super) fn from_entry(entry: &InteractiveEntryOptions) -> Self {
        let mut transcript = Transcript::new();
        transcript.append_root_block(
            NewBlock::new(BlockKind::SystemNote, "fullscreen foundation").with_content(
                "Fullscreen transcript foundation active. This path owns the terminal with an alternate screen, raw mode, mouse capture, a fixed bottom input box, a dedicated transcript viewport, a dedicated status line, and a shared structured transcript block model.",
            ),
        );

        if !entry.messages.is_empty() {
            transcript.append_root_block(
                NewBlock::new(BlockKind::SystemNote, "startup messages").with_content(format!(
                    "Loaded {} startup message(s) into the shared fullscreen transcript shell. Agent turn execution stays on the legacy interactive path for now.",
                    entry.messages.len()
                )),
            );

            for message in &entry.messages {
                transcript.append_root_block(
                    NewBlock::new(BlockKind::UserMessage, "startup prompt")
                        .with_content(message.clone()),
                );
            }
        }

        let config = FullscreenAppConfig {
            title: "BB-Agent fullscreen transcript".to_string(),
            input_placeholder: "Type a prompt in the new fullscreen shell…".to_string(),
            status_line: "Esc quits • Enter captures the prompt locally • Shift+Enter inserts a newline • wheel scrolls transcript"
                .to_string(),
            transcript,
        };

        Self { config }
    }

    pub(super) fn into_config(self) -> FullscreenAppConfig {
        self.config
    }
}
