use std::{
    fs::{File, OpenOptions},
    io, iter,
    num::NonZeroU32,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
        mpsc::{SendError, Sender},
    },
    time::{
        Duration,
        Instant,
    },
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

use size_format::SizeFormatterBinary;

use thiserror::Error;

use crate::{
    config::Config,
    ipc::{Message, ProgressMessage},
    memslot::ReadHalf,
    progress::ProgressView,
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
    progress: Arc<AtomicUsize>,
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
        progress: Arc<AtomicUsize>,
    ) -> Result<Self> {
        let backend = Self::initialize_backend()?;
        let terminal = Terminal::new(backend)?;
        Ok(Self {
            terminal,
            rx,
            tx,
            progress,
        })
    }
    fn initialize_backend() -> Result<CrosstermBackend<File>> {
        let mut tty =
            OpenOptions::new().read(true).write(true).open("/dev/tty")?;
        terminal::enable_raw_mode()?;
        execute!(tty, terminal::EnterAlternateScreen)?;
        Ok(CrosstermBackend::new(tty))
    }
    pub fn run(mut self, start_time: Instant) -> Result<Cleanup> {
        let events = iter::once(Event::Tick).chain(Events);
        self.terminal.clear()?;
        for event in events {
            match event {
                Event::Input(InputEvent::Key(KeyEvent {
                    code: KeyCode::Left,
                    ..
                })) => {
                    self.decrease_rate();
                }
                Event::Input(InputEvent::Key(KeyEvent {
                    code: KeyCode::Right,
                    ..
                })) => {
                    self.increase_rate();
                }
                Event::Input(InputEvent::Key(KeyEvent {
                    code: KeyCode::Char(' '),
                    ..
                })) => {
                    self.toggle_paused()?;
                }
                Event::Input(InputEvent::Key(KeyEvent {
                    code: KeyCode::Char('c'),
                    modifiers: KeyModifiers::CONTROL,
                })) => {
                    self.tx.send(Message::Interrupted)?;
                    break;
                }
                _ => {}
            }
            if let ProgressMessage::Interrupted = self.rx.get() {
                break;
            }
            let progress_view = ProgressView {
                bytes_transferred: self.progress.load(Ordering::Relaxed),
                start_time,
                ..Default::default()
            };
            self.terminal.draw(|f| Self::draw(f, progress_view))?;
        }
        Ok(Cleanup())
    }

    fn toggle_paused(&mut self) -> Result<()> {
        let config = Config::current();
        Config {
            paused: !config.paused,
            ..config
        }.make_current();
        Ok(())
    }

    fn increase_rate(&mut self) {
        let config = Config::current();
        let limit = checked_add(config.limit(), 10);
        Config {
            limit: Some(limit),
            ..config
        }.make_current();
    }

    fn decrease_rate(&mut self) {
        let config = Config::current();
        let limit = checked_sub(config.limit(), 10);
        Config {
            limit: Some(limit),
            ..config
        }.make_current();
    }

    fn draw<B: Backend>(f: &mut Frame<B>, progress: ProgressView) {
        let config = Config::current();

        let pause = if config.paused { " [PAUSED]" } else { "" };
        let limit = SizeFormatterBinary::new(config.limit().get() as u64);
        let ProgressView {
            recent_throughput,
            bytes_transferred,
            ..
        } = progress;

        let para = format!(
            "{:.2}B {} [{:.2}B/s / {:.2}B/s measured] {}",
            SizeFormatterBinary::new(bytes_transferred as u64),
            format_duration(progress.elapsed()),
            limit,
            SizeFormatterBinary::new(recent_throughput as u64),
            pause,
        );

        let size = f.size();

        let row = Rect::new(0, 0, size.width, 1);

        f.render_widget(Paragraph::new(para), row);
    }
}

fn format_duration(duration: Duration) -> String {
    let duration = chrono::Duration::from_std(duration).unwrap();
    format!(
        "{}:{:02}:{:02}",
        duration.num_hours(),
        duration.num_minutes() % 60,
        duration.num_seconds() % 60,
    )
}

impl Drop for UserInterface {
    fn drop(&mut self) {
        Cleanup();
    }
}
