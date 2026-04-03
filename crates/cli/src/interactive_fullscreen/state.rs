use bb_tui::fullscreen::{FullscreenAppConfig, TranscriptItem, TranscriptRole};

use crate::interactive::InteractiveEntryOptions;

pub(super) struct InteractiveFullscreenState {
    config: FullscreenAppConfig,
}

impl InteractiveFullscreenState {
    pub(super) fn from_entry(entry: &InteractiveEntryOptions) -> Self {
        let mut transcript = vec![TranscriptItem::new(
            TranscriptRole::System,
            "Fullscreen transcript foundation active. This path owns the terminal with an alternate screen, raw mode, mouse capture, a fixed bottom input box, a dedicated transcript viewport, and a dedicated status line.",
        )];

        if !entry.messages.is_empty() {
            transcript.push(TranscriptItem::new(
                TranscriptRole::Status,
                format!(
                    "Loaded {} startup message(s) into the transcript shell. Agent turn execution stays on the legacy interactive path for now.",
                    entry.messages.len()
                ),
            ));

            transcript.extend(
                entry
                    .messages
                    .iter()
                    .cloned()
                    .map(|message| TranscriptItem::new(TranscriptRole::User, message)),
            );
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
