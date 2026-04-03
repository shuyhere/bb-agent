use std::io;
use std::time::Duration;

use super::{
    frame::build_frame,
    layout::Size,
    renderer::FullscreenRenderer,
    state::{FullscreenAppConfig, FullscreenOutcome, FullscreenState},
    terminal::{FullscreenEvent, FullscreenTerminal, spawn_event_reader},
};

pub async fn run(config: FullscreenAppConfig) -> io::Result<FullscreenOutcome> {
    let mut terminal = FullscreenTerminal::enter()?;
    let (width, height) = terminal.size()?;
    let mut state = FullscreenState::new(config, Size { width, height });
    let mut renderer = FullscreenRenderer::new();
    let mut events = spawn_event_reader();
    let mut tick = tokio::time::interval(Duration::from_millis(100));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    if state.take_dirty() {
        let frame = build_frame(&state);
        renderer.render(&mut terminal, &frame)?;
    }

    loop {
        if state.should_quit {
            break;
        }

        tokio::select! {
            maybe_event = events.recv() => {
                let Some(event) = maybe_event else {
                    break;
                };

                match event {
                    FullscreenEvent::Key(key) => state.on_key(key),
                    FullscreenEvent::Mouse(mouse) => state.on_mouse(mouse),
                    FullscreenEvent::Resize(width, height) => state.on_resize(width, height),
                    FullscreenEvent::Paste(text) => state.on_paste(&text),
                }
            }
            _ = tick.tick() => {
                state.on_tick();
            }
        }

        if state.take_dirty() {
            let frame = build_frame(&state);
            renderer.render(&mut terminal, &frame)?;
        }
    }

    Ok(state.outcome())
}
