use nonzero_ext::nonzero;
use std::{
    fs::{File, OpenOptions},
    io, iter,
    num::NonZeroU32,
    sync::mpsc::{SendError, Sender},
    time::Duration,
};

use tui::{
    backend::{Backend, CrosstermBackend},
    layout::Rect,
    widgets::Paragraph,
    Frame, Terminal,
};

use crossterm::{
    event::{poll, read, Event as InputEvent, KeyCode, KeyEvent, KeyModifiers},
    execute, terminal,
};

use thiserror::Error;

use crate::{
    ipc::{Message, ProgressMessage},
    memslot::ReadHalf,
};

#[derive(Debug, Error)]
pub enum UserInterfaceError {
    #[error("I/O error talking to terminal")]
    IO(#[from] io::Error),
    #[error("I/O error talking to terminal")]
    Crossterm(#[from] crossterm::ErrorKind),
    #[error("Failed to send control message to stream.")]
    IPC(#[from] SendError<Message>),
}

type Result<T> = std::result::Result<T, UserInterfaceError>;

#[derive(Debug)]
enum Event {
    Tick,
    Input(InputEvent),
}

struct Events;

impl Iterator for Events {
    type Item = Event;

    fn next(&mut self) -> Option<Self::Item> {
        match poll(Duration::from_secs(1)) {
            Ok(true) => {
                let event = read().unwrap();
                Some(Event::Input(event))
            }
            Ok(false) => Some(Event::Tick),
            _ => unreachable!("failed to iterate input events"),
        }
    }
}

fn checked_add(value: NonZeroU32, increment: u32) -> NonZeroU32 {
    value
        .get()
        .checked_add(increment)
        .map(|n| n - (n % increment))
        .map(|n| 1.max(n))
        .and_then(NonZeroU32::new)
        .unwrap_or(value)
}

fn checked_sub(value: NonZeroU32, increment: u32) -> NonZeroU32 {
    value
        .get()
        .checked_sub(increment)
        .map(|n| n - (n % increment))
        .map(|n| 1.max(n))
        .and_then(NonZeroU32::new)
        .unwrap_or(value)
}

type CrossTerminal = Terminal<CrosstermBackend<File>>;

pub struct UserInterface {
    terminal: CrossTerminal,
    tx: Sender<Message>,
    rx: ReadHalf<ProgressMessage>,
}

pub struct Cleanup();

impl Drop for Cleanup {
    fn drop(&mut self) {
        if let Ok(mut tty) =
            OpenOptions::new().read(true).write(true).open("/dev/tty")
        {
            execute!(tty, terminal::LeaveAlternateScreen)
                .expect("failed to leave alternate screen");
            terminal::disable_raw_mode().expect("failed to disable raw mode");
        }
    }
}

impl UserInterface {
    pub fn new(
        tx: Sender<Message>,
        rx: ReadHalf<ProgressMessage>,
    ) -> Result<Self> {
        let backend = UserInterface::initialize_backend()?;
        let terminal = Terminal::new(backend)?;
        Ok(Self { terminal, rx, tx })
    }
    fn initialize_backend() -> Result<CrosstermBackend<File>> {
        let mut tty =
            OpenOptions::new().read(true).write(true).open("/dev/tty")?;
        terminal::enable_raw_mode()?;
        execute!(tty, terminal::EnterAlternateScreen)?;
        Ok(CrosstermBackend::new(tty))
    }
    pub fn run(mut self) -> Result<Cleanup> {
        let mut rate = nonzero!(1u32);
        let events = iter::once(Event::Tick).chain(Events);
        let terminal = &mut self.terminal;
        let tx = &mut self.tx;
        let rx = &mut self.rx;

        let mut bytes_transferred = 0;
        let mut duration = Duration::from_secs(0);

        terminal.clear()?;
        for event in events {
            match event {
                Event::Input(InputEvent::Key(KeyEvent {
                    code: KeyCode::Left,
                    modifiers: _,
                })) => {
                    rate = checked_sub(rate, 10);
                    tx.send(Message::UpdateRate(rate))?;
                }
                Event::Input(InputEvent::Key(KeyEvent {
                    code: KeyCode::Right,
                    modifiers: _,
                })) => {
                    rate = checked_add(rate, 10);
                    tx.send(Message::UpdateRate(rate))?;
                }
                Event::Input(InputEvent::Key(KeyEvent {
                    code: KeyCode::Char('c'),
                    modifiers: KeyModifiers::CONTROL,
                })) => {
                    tx.send(Message::Interrupted)?;
                    break;
                }
                _ => {}
            }
            match rx.get() {
                ProgressMessage::Interrupted => {
                    break;
                }
                ProgressMessage::Transfer(bytes, age) => {
                    bytes_transferred = bytes;
                    duration = age;
                }
                _ => {}
            }
            terminal.draw(|f| {
                Self::draw(f, rate.clone(), bytes_transferred, duration)
            })?;
        }
        Ok(Cleanup())
    }

    fn draw<B: Backend>(
        f: &mut Frame<B>,
        rate: NonZeroU32,
        bytes_transferred: usize,
        duration: Duration,
    ) {
        let para = Paragraph::new(format!(
            "{:?} {} copying at {} bytes/sec",
            duration, bytes_transferred, rate,
        ));

        let size = f.size();

        let row = Rect::new(0, 0, size.width, 1);

        f.render_widget(para, row);
    }
}

impl Drop for UserInterface {
    fn drop(&mut self) {
        Cleanup();
    }
}
