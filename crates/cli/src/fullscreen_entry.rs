use anyhow::Result;
use bb_tui::fullscreen::{BlockKind, FullscreenAppConfig, NewBlock, Transcript};

use crate::interactive::InteractiveEntryOptions;

/// Thin CLI bootstrap onto the shared fullscreen transcript runtime.
///
/// Keep entry-specific defaults here only. Transcript state, rendering, input,
/// and terminal ownership belong in `bb_tui::fullscreen` so the final cutover
/// lands on one shared implementation instead of another CLI-local fork.
pub async fn run_fullscreen_entry(entry: InteractiveEntryOptions) -> Result<()> {
    let _ = bb_tui::fullscreen::run(build_fullscreen_config(&entry)).await?;
    Ok(())
}

fn build_fullscreen_config(entry: &InteractiveEntryOptions) -> FullscreenAppConfig {
    let mut transcript = Transcript::new();
    transcript.append_root_block(
        NewBlock::new(BlockKind::SystemNote, "fullscreen foundation").with_content(
            "Shared fullscreen transcript foundation active. This path owns the terminal with an alternate screen, raw mode, mouse capture, a fixed bottom input box, a dedicated transcript viewport, a dedicated status line, and the shared structured transcript block model.",
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

    FullscreenAppConfig {
        title: "BB-Agent fullscreen transcript".to_string(),
        input_placeholder: "Type a prompt in the shared fullscreen shell…".to_string(),
        status_line:
            "Ctrl+O transcript • Enter captures the prompt locally • Shift+Enter inserts a newline • wheel scrolls transcript"
                .to_string(),
        transcript,
    }
}
