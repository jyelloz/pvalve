use std::{
    fs::{File, OpenOptions},
    io, iter,
    num::NonZeroU32,
    sync::mpsc::{SendError, Sender},
    time::{
        Duration,
        Instant,
    },
};

use tui::{
    backend::{Backend, CrosstermBackend},
    layout::{
        Rect,
        Layout,
        Constraint,
        Direction,
    },
    widgets::Paragraph,
    style::{
        Style,
        Color,
        Modifier,
    },
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
    progress::{
        ProgressView,
        ProgressCounter,
    },
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

#[derive(Debug, Clone, Copy)]
enum Mode {
    Progress,
    Edit,
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
    progress: ProgressCounter,
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
        progress: ProgressCounter,
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
        let mut mode = Mode::Progress;
        let mut input = String::new();
        self.terminal.clear()?;
        for event in events {
            match mode {
                Mode::Progress => match event {
                    Event::Input(InputEvent::Key(KeyEvent {
                        code: KeyCode::Char('e'),
                        ..
                    })) => {
                        mode = Mode::Edit;
                    },
                    Event::Input(InputEvent::Key(KeyEvent {
                        code: KeyCode::Left,
                        ..
                    })) => {
                        self.decrease_rate();
                    },
                    Event::Input(InputEvent::Key(KeyEvent {
                        code: KeyCode::Right,
                        ..
                    })) => {
                        self.increase_rate();
                    },
                    Event::Input(InputEvent::Key(KeyEvent {
                        code: KeyCode::Char(' '),
                        ..
                    })) => {
                        self.toggle_paused()?;
                    },
                    Event::Input(InputEvent::Key(KeyEvent {
                        code: KeyCode::Char('c'),
                        modifiers: KeyModifiers::CONTROL,
                    })) => {
                        self.tx.send(Message::Interrupted)?;
                        break;
                    },
                    _ => {},
                },
                Mode::Edit => match event {
                    Event::Input(InputEvent::Key(KeyEvent {
                        code: KeyCode::Esc,
                        ..
                    })) => {
                        input.clear();
                        mode = Mode::Progress;
                    },
                    Event::Input(InputEvent::Key(KeyEvent {
                        code: KeyCode::Char(code @ '0'..='9'),
                        ..
                    })) => {
                        input.push(code);
                    },
                    Event::Input(InputEvent::Key(KeyEvent {
                        code: KeyCode::Backspace,
                        ..
                    })) => {
                        input.pop();
                    },
                    Event::Input(InputEvent::Key(KeyEvent {
                        code: KeyCode::Enter,
                        ..
                    })) => {
                        let new_rate = u32::from_str_radix(&input, 10)
                            .ok()
                            .and_then(NonZeroU32::new);
                        input.clear();
                        if let Some(new_rate) = new_rate {
                            self.set_limit(new_rate);
                        }
                        mode = Mode::Progress;
                    },
                    _ => {},
                },
            }
            if let ProgressMessage::Interrupted = self.rx.get() {
                break;
            }
            let progress_view = ProgressView {
                bytes_transferred: self.progress.get(),
                start_time,
                ..Default::default()
            };
            let config = Config::current();
            self.terminal.draw(|f| Self::draw(
                    f,
                    mode,
                    config,
                    progress_view,
                    &input,
            ))?;
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

    fn set_limit(&mut self, limit: NonZeroU32) {
        let config = Config::current();
        Config {
            limit: Some(limit),
            ..config
        }.make_current();
    }

    fn increase_rate(&mut self) {
        let config = Config::current();
        let limit = checked_add(config.limit(), 10);
        self.set_limit(limit);
    }

    fn decrease_rate(&mut self) {
        let config = Config::current();
        let limit = checked_sub(config.limit(), 10);
        self.set_limit(limit);
    }

    fn draw<B: Backend>(
        f: &mut Frame<B>,
        mode: Mode,
        config: Config,
        progress: ProgressView,
        input: &str,
    ) {
        match mode {
            Mode::Progress => Self::draw_progress_mode(f, config, progress),
            Mode::Edit => Self::draw_update_mode(f, &input),
        }
    }

    fn draw_progress_mode<B: Backend>(
        f: &mut Frame<B>,
        config: Config,
        progress: ProgressView
    ) {

        let pause = if config.paused { "[PAUSED]" } else { "" };
        let limit = SizeFormatterBinary::new(config.limit().get() as u64);
        let ProgressView {
            bytes_transferred,
            ..
        } = progress;

        let para = format!(
            "{:.2}B {} [{:.2}B/s]",
            SizeFormatterBinary::new(bytes_transferred as u64),
            format_duration(progress.elapsed()),
            limit,
        );

        let row = Rect {
            height: 1,
            ..f.size()
        };

        let layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(para.len() as u16),
                Constraint::Length(1),
                Constraint::Length(pause.len() as u16),
            ])
            .split(row);

        let progress = Paragraph::new(para);
        let pause = Paragraph::new(pause)
            .style(Style::default().add_modifier(Modifier::RAPID_BLINK));

        if let [l, _, r] = *layout {
            f.render_widget(progress, l);
            f.render_widget(pause, r);
        }

    }

    fn draw_update_mode<B: Backend>(f: &mut Frame<B>, input: &str) {
        let row = Rect {
            height: 1,
            ..f.size()
        };
        let message = "enter a new rate:";
        let layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(
                [
                    Constraint::Length(message.len() as u16),
                    Constraint::Length(1),
                    Constraint::Min(10),
                ]
            )
            .split(row);
        let para = Paragraph::new(message)
            .style(
                Style::default()
                .bg(Color::Blue)
                .fg(Color::White)
            );
        let input_length = input.len() as u16;
        let input = Paragraph::new(input)
            .style(Style::default().add_modifier(Modifier::BOLD));
        if let [l, _, r] = *layout {
            f.set_cursor(r.x + input_length, r.y);
            f.render_widget(para, l);
            f.render_widget(input, r);
        }
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
