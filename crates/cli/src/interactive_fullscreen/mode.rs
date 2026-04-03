use anyhow::Result;

use crate::interactive::InteractiveEntryOptions;

use super::state::InteractiveFullscreenState;

pub async fn run_interactive_fullscreen(entry: InteractiveEntryOptions) -> Result<()> {
    let shell_state = InteractiveFullscreenState::from_entry(&entry);
    let _ = bb_tui::fullscreen::run(shell_state.into_config()).await?;
    Ok(())
}
