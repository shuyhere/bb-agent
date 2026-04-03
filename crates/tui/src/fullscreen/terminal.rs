use crossterm::{
    cursor,
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyEvent, MouseEvent},
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use std::io::{self, Write};
use tokio::sync::mpsc;

const SYNC_BEGIN: &str = "\x1b[?2026h";
const SYNC_END: &str = "\x1b[?2026l";

#[derive(Debug, Clone)]
pub enum FullscreenEvent {
    Key(KeyEvent),
    Mouse(MouseEvent),
    Resize(u16, u16),
    Paste(String),
}

pub struct FullscreenTerminal {
    stdout: io::Stdout,
    active: bool,
    sync_updates_supported: bool,
}

impl FullscreenTerminal {
    pub fn enter() -> io::Result<Self> {
        terminal::enable_raw_mode()?;

        let mut stdout = io::stdout();
        execute!(
            stdout,
            EnterAlternateScreen,
            EnableMouseCapture,
            cursor::Hide,
        )?;
        write!(stdout, "\x1b[?2004h")?;
        stdout.flush()?;

        Ok(Self {
            stdout,
            active: true,
            sync_updates_supported: sync_updates_supported(),
        })
    }

    pub fn size(&self) -> io::Result<(u16, u16)> {
        terminal::size()
    }

    pub fn sync_updates_supported(&self) -> bool {
        self.sync_updates_supported
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

        write!(self.stdout, "\x1b[?2004l")?;
        execute!(
            self.stdout,
            cursor::Show,
            DisableMouseCapture,
            LeaveAlternateScreen,
        )?;
        self.stdout.flush()?;
        terminal::disable_raw_mode()?;
        self.active = false;
        Ok(())
    }
}

impl Drop for FullscreenTerminal {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

pub fn spawn_event_reader() -> mpsc::UnboundedReceiver<FullscreenEvent> {
    let (tx, rx) = mpsc::unbounded_channel();

    std::thread::spawn(move || {
        loop {
            match event::read() {
                Ok(Event::Key(key)) => {
                    if tx.send(FullscreenEvent::Key(key)).is_err() {
                        break;
                    }
                }
                Ok(Event::Mouse(mouse)) => {
                    if tx.send(FullscreenEvent::Mouse(mouse)).is_err() {
                        break;
                    }
                }
                Ok(Event::Resize(width, height)) => {
                    if tx.send(FullscreenEvent::Resize(width, height)).is_err() {
                        break;
                    }
                }
                Ok(Event::Paste(text)) => {
                    if tx.send(FullscreenEvent::Paste(text)).is_err() {
                        break;
                    }
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
    });

    rx
}

fn sync_updates_supported() -> bool {
    if std::env::var("BB_DISABLE_SYNC_UPDATES").as_deref() == Ok("1") {
        return false;
    }

    std::env::var("TERM")
        .map(|term| !term.eq_ignore_ascii_case("dumb"))
        .unwrap_or(true)
}
