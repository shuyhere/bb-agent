use std::io;
use std::time::{Duration, Instant};

use tokio::sync::mpsc;

use crate::tui::frame::build_frame;
use crate::tui::renderer::TuiRenderer;
use crate::tui::scheduler::{RenderIntent, RenderScheduler};
use crate::tui::terminal::{TuiEvent, TuiTerminal, spawn_event_reader};
use crate::tui::types::{
    TuiAppConfig, TuiCommand, TuiOutcome, TuiSubmission,
};

use super::{TuiState, Size};

pub async fn run(config: TuiAppConfig) -> io::Result<TuiOutcome> {
    let (_command_tx, command_rx) = mpsc::unbounded_channel();
    let (submission_tx, _submission_rx) = mpsc::unbounded_channel();
    run_with_channels(config, command_rx, submission_tx).await
}

pub async fn run_with_channels(
    config: TuiAppConfig,
    mut command_rx: mpsc::UnboundedReceiver<TuiCommand>,
    submission_tx: mpsc::UnboundedSender<TuiSubmission>,
) -> io::Result<TuiOutcome> {
    let mut terminal = TuiTerminal::enter()?;
    let (width, height) = terminal.size()?;
    let mut state = TuiState::new(config, Size { width, height });
    let mut renderer = TuiRenderer::new();
    let mut events = spawn_event_reader();
    let mut scheduler = RenderScheduler::default();
    let mut command_open = true;
    let mut tick = tokio::time::interval(Duration::from_millis(80));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    render_now(&mut terminal, &mut renderer, &mut state)?;
    apply_pending_terminal_state(&mut terminal, &mut state)?;
    flush_submissions(&mut state, &submission_tx);

    loop {
        if state.should_quit {
            break;
        }

        let scheduled_deadline = scheduler.next_flush_at();
        let scheduled_flush = async move {
            match scheduled_deadline {
                Some(deadline) => {
                    tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)).await;
                }
                None => std::future::pending::<()>().await,
            }
        };
        tokio::pin!(scheduled_flush);

        tokio::select! {
            maybe_event = events.recv() => {
                let Some(event) = maybe_event else {
                    break;
                };

                match event {
                    TuiEvent::Key(key) => state.on_key(key),
                    TuiEvent::Mouse(mouse) => state.on_mouse(mouse),
                    TuiEvent::Resize(width, height) => state.on_resize(width, height),
                    TuiEvent::Paste(text) => state.on_paste(&text),
                }

                if state.dirty {
                    render_now(&mut terminal, &mut renderer, &mut state)?;
                    scheduler.clear();
                }
                apply_pending_terminal_state(&mut terminal, &mut state)?;
                flush_submissions(&mut state, &submission_tx);
            }
            maybe_command = command_rx.recv(), if command_open => {
                match maybe_command {
                    Some(command) => apply_render_intent(
                        state.apply_command(command),
                        &mut scheduler,
                        &mut terminal,
                        &mut renderer,
                        &mut state,
                    )?,
                    None => {
                        command_open = false;
                        state.should_quit = true;
                        state.dirty = true;
                    }
                }
            }
            _ = &mut scheduled_flush => {
                if scheduler.should_flush(Instant::now()) {
                    render_now(&mut terminal, &mut renderer, &mut state)?;
                    scheduler.on_flushed();
                }
            }
            _ = tick.tick() => {
                state.on_tick();
                if state.dirty {
                    render_now(&mut terminal, &mut renderer, &mut state)?;
                    scheduler.clear();
                }
            }
        }

        apply_pending_terminal_state(&mut terminal, &mut state)?;
        flush_submissions(&mut state, &submission_tx);
    }

    if scheduler.is_dirty() || state.dirty {
        render_now(&mut terminal, &mut renderer, &mut state)?;
        scheduler.on_flushed();
    }
    apply_pending_terminal_state(&mut terminal, &mut state)?;

    Ok(state.outcome())
}

fn base64_encode_simple(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    let mut i = 0;
    while i < data.len() {
        let b0 = data[i] as u32;
        let b1 = if i + 1 < data.len() {
            data[i + 1] as u32
        } else {
            0
        };
        let b2 = if i + 2 < data.len() {
            data[i + 2] as u32
        } else {
            0
        };
        let n = (b0 << 16) | (b1 << 8) | b2;
        let c0 = ((n >> 18) & 0x3F) as usize;
        let c1 = ((n >> 12) & 0x3F) as usize;
        let c2 = ((n >> 6) & 0x3F) as usize;
        let c3 = (n & 0x3F) as usize;
        out.push(TABLE[c0] as char);
        out.push(TABLE[c1] as char);
        out.push(if i + 1 < data.len() {
            TABLE[c2] as char
        } else {
            '='
        });
        out.push(if i + 2 < data.len() {
            TABLE[c3] as char
        } else {
            '='
        });
        i += 3;
    }
    out
}

#[cfg(all(unix, not(target_os = "macos")))]
fn is_ssh_session() -> bool {
    std::env::var_os("SSH_CONNECTION").is_some()
        || std::env::var_os("SSH_CLIENT").is_some()
        || std::env::var_os("SSH_TTY").is_some()
}

#[cfg(all(unix, not(target_os = "macos")))]
fn linux_gui_clipboard_available() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some() || std::env::var_os("DISPLAY").is_some()
}

fn copy_via_spawned_stdin(cmd: &str, args: &[&str], text: &str) -> bool {
    let spawned = std::process::Command::new(cmd)
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
    let Ok(mut child) = spawned else {
        return false;
    };
    use std::io::Write;
    if let Some(stdin) = child.stdin.as_mut() {
        let _ = stdin.write_all(text.as_bytes());
    }
    child.wait().map(|status| status.success()).unwrap_or(false)
}

fn copy_text_to_system_clipboard(text: &str) {
    #[cfg(target_os = "macos")]
    {
        let _ = copy_via_spawned_stdin("pbcopy", &[], text);
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if is_ssh_session() || !linux_gui_clipboard_available() {
            return;
        }

        for cmd in [
            ("wl-copy", Vec::<&str>::new()),
            ("xclip", vec!["-selection", "clipboard"]),
        ] {
            if copy_via_spawned_stdin(cmd.0, &cmd.1, text) {
                break;
            }
        }
    }
}

fn apply_pending_terminal_state(
    terminal: &mut TuiTerminal,
    state: &mut TuiState,
) -> io::Result<()> {
    if let Some(text) = state.take_pending_clipboard_copy() {
        copy_text_to_system_clipboard(&text);
        let encoded = base64_encode_simple(text.as_bytes());
        terminal.write_raw(&format!("\x1b]52;c;{encoded}\x07"))?;
    }
    Ok(())
}

fn apply_render_intent(
    intent: RenderIntent,
    scheduler: &mut RenderScheduler,
    terminal: &mut TuiTerminal,
    renderer: &mut TuiRenderer,
    state: &mut TuiState,
) -> io::Result<()> {
    match intent {
        RenderIntent::None => {}
        RenderIntent::Schedule => scheduler.mark_dirty(Instant::now()),
        RenderIntent::Render => {
            render_now(terminal, renderer, state)?;
            scheduler.on_flushed();
        }
    }
    Ok(())
}

fn render_now(
    terminal: &mut TuiTerminal,
    renderer: &mut TuiRenderer,
    state: &mut TuiState,
) -> io::Result<()> {
    if !state.take_dirty() {
        return Ok(());
    }
    if state.force_full_repaint {
        renderer.invalidate();
        state.force_full_repaint = false;
    }
    state.prepare_for_render();
    let frame = build_frame(state);
    renderer.render(terminal, &frame)
}

fn flush_submissions(
    state: &mut TuiState,
    submission_tx: &mpsc::UnboundedSender<TuiSubmission>,
) {
    for submitted in state.take_pending_submissions() {
        if submission_tx.send(submitted).is_err() {
            break;
        }
    }
}
