use crossterm::{
    cursor,
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyEvent, MouseEvent, MouseEventKind,
    },
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use std::{
    io::{self, Write},
    time::Duration,
};
use tokio::sync::mpsc;

const SYNC_BEGIN: &str = "\x1b[?2026h";
const SYNC_END: &str = "\x1b[?2026l";

fn write_emergency_restore_sequences(stdout: &mut io::Stdout) -> io::Result<()> {
    // Disable bracketed paste, sync updates, common xterm/tmux mouse tracking modes,
    // and make sure the cursor is visible again.
    write!(
        stdout,
        "\x1b[?2004l\x1b[?2026l\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1005l\x1b[?1006l\x1b[?1015l\x1b[?25h"
    )?;
    stdout.flush()
}

#[derive(Debug, Clone)]
pub enum TuiEvent {
    Key(KeyEvent),
    Mouse(MouseEvent),
    Resize(u16, u16),
    Paste(String),
}

pub struct TuiTerminal {
    stdout: io::Stdout,
    active: bool,
    sync_updates_supported: bool,
    mouse_capture_enabled: bool,
}

impl TuiTerminal {
    pub fn enter() -> io::Result<Self> {
        let mut stdout = io::stdout();
        let _ = write_emergency_restore_sequences(&mut stdout);
        let _ = terminal::disable_raw_mode();

        terminal::enable_raw_mode()?;
        execute!(
            stdout,
            EnterAlternateScreen,
            EnableMouseCapture,
            cursor::Hide
        )?;
        write!(stdout, "\x1b[?2004h")?;
        stdout.flush()?;

        Ok(Self {
            stdout,
            active: true,
            sync_updates_supported: sync_updates_supported(),
            mouse_capture_enabled: true,
        })
    }

    pub fn size(&self) -> io::Result<(u16, u16)> {
        terminal::size()
    }

    pub fn write_raw(&mut self, data: &str) -> io::Result<()> {
        self.stdout.write_all(data.as_bytes())?;
        self.stdout.flush()
    }

    pub fn begin_sync(&self, buf: &mut String) {
        if self.sync_updates_supported {
            buf.push_str(SYNC_BEGIN);
        }
    }

    pub fn end_sync(&self, buf: &mut String) {
        if self.sync_updates_supported {
            buf.push_str(SYNC_END);
        }
    }

    pub fn restore(&mut self) -> io::Result<()> {
        if !self.active {
            return Ok(());
        }

        let _ = write_emergency_restore_sequences(&mut self.stdout);
        let _ = execute!(
            self.stdout,
            DisableMouseCapture,
            cursor::Show,
            LeaveAlternateScreen
        );
        let _ = self.stdout.flush();
        let _ = terminal::disable_raw_mode();
        self.active = false;
        self.mouse_capture_enabled = false;
        Ok(())
    }
}

impl Drop for TuiTerminal {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

pub fn spawn_event_reader() -> mpsc::UnboundedReceiver<TuiEvent> {
    let (tx, rx) = mpsc::unbounded_channel();

    std::thread::spawn(move || {
        while let Ok(event) = event::read() {
            if let Event::Mouse(mouse) = event {
                if matches!(mouse.kind, MouseEventKind::Drag(_)) {
                    let mut latest_drag = mouse;
                    loop {
                        match event::poll(Duration::from_millis(0)) {
                            Ok(true) => match event::read() {
                                Ok(Event::Mouse(next_mouse))
                                    if matches!(next_mouse.kind, MouseEventKind::Drag(_)) =>
                                {
                                    latest_drag = next_mouse;
                                }
                                Ok(other) => {
                                    if !send_tui_event(
                                        &tx,
                                        TuiEvent::Mouse(latest_drag),
                                    ) {
                                        return;
                                    }
                                    if !forward_event(&tx, other) {
                                        return;
                                    }
                                    break;
                                }
                                Err(_) => return,
                            },
                            Ok(false) => {
                                if !send_tui_event(&tx, TuiEvent::Mouse(latest_drag))
                                {
                                    return;
                                }
                                break;
                            }
                            Err(_) => return,
                        }
                    }
                    continue;
                }

                if !send_tui_event(&tx, TuiEvent::Mouse(mouse)) {
                    break;
                }
                continue;
            }

            if !forward_event(&tx, event) {
                break;
            }
        }
    });

    rx
}

fn forward_event(tx: &mpsc::UnboundedSender<TuiEvent>, event: Event) -> bool {
    match event {
        Event::Key(key) => send_tui_event(tx, TuiEvent::Key(key)),
        Event::Mouse(mouse) => send_tui_event(tx, TuiEvent::Mouse(mouse)),
        Event::Resize(width, height) => {
            send_tui_event(tx, TuiEvent::Resize(width, height))
        }
        Event::Paste(text) => send_tui_event(tx, TuiEvent::Paste(text)),
        _ => true,
    }
}

fn send_tui_event(
    tx: &mpsc::UnboundedSender<TuiEvent>,
    event: TuiEvent,
) -> bool {
    tx.send(event).is_ok()
}

fn sync_updates_supported() -> bool {
    if std::env::var("BB_DISABLE_SYNC_UPDATES").as_deref() == Ok("1") {
        return false;
    }

    std::env::var("TERM")
        .map(|term| !term.eq_ignore_ascii_case("dumb"))
        .unwrap_or(true)
}
