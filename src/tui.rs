use std::{
    fs::{File, OpenOptions},
    io, iter,
    num::NonZeroU32,
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

use watch::WatchSender;

use super::{
    config::{Config, Latch, LatchMonitor, Unit},
    progress::{
        TransferProgress,
        TransferProgressMonitor,
    }
};

#[derive(Debug, Error)]
pub enum UserInterfaceError {
    #[error("I/O error talking to terminal")]
    IO(#[from] io::Error),
    #[error("I/O error talking to terminal")]
    Crossterm(#[from] crossterm::ErrorKind),
}

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Eq, Ord)]
enum TuiMode {
    Progress,
    Edit,
}

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Eq, Ord)]
struct TransferMode {
    paused: bool,
    limit: NonZeroU32,
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

fn checked_add(value: Option<NonZeroU32>, increment: u32) -> Option<NonZeroU32> {
    if let Some(value) = value {
        value
            .get()
            .checked_add(increment)
            .map(|n| n - (n % increment))
            .map(|n| 1.max(n))
            .and_then(NonZeroU32::new)
    } else {
        None
    }
}

fn checked_sub(value: Option<NonZeroU32>, increment: u32) -> Option<NonZeroU32> {
    if let Some(value) = value {
        value
            .get()
            .checked_sub(increment)
            .map(|n| n - (n % increment))
            .map(|n| 1.max(n))
            .and_then(NonZeroU32::new)
    } else {
        None
    }
}

type CrossTerminal = Terminal<CrosstermBackend<File>>;

pub struct UserInterface {
    terminal: CrossTerminal,
    shutdown: LatchMonitor,
    config: Config,
    config_tx: WatchSender<Config>,
    paused: Latch,
    aborted: Latch,
    progress: TransferProgressMonitor,
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
        paused: Latch,
        aborted: Latch,
        shutdown: LatchMonitor,
        config: Config,
        progress: TransferProgressMonitor,
        config_tx: WatchSender<Config>,
    ) -> Result<Self> {
        let backend = Self::initialize_backend()?;
        let terminal = Terminal::new(backend)?;
        Ok(Self {
            terminal,
            shutdown,
            config,
            progress,
            config_tx,
            paused,
            aborted,
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
        let mut mode = TuiMode::Progress;
        let mut input = String::new();
        self.terminal.clear()?;
        for event in events {
            match mode {
                TuiMode::Progress => match event {
                    Event::Input(InputEvent::Key(KeyEvent {
                        code: KeyCode::Char('e'),
                        ..
                    })) => {
                        mode = TuiMode::Edit;
                    },
                    Event::Input(InputEvent::Key(KeyEvent {
                        code: KeyCode::Tab,
                        ..
                    })) => { self.cycle_unit(); },
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
                        self.toggle_paused();
                    },
                    Event::Input(InputEvent::Key(KeyEvent {
                        code: KeyCode::Char('c'),
                        modifiers: KeyModifiers::CONTROL,
                    })) => {
                        self.aborted.on();
                        break;
                    },
                    _ => {},
                },
                TuiMode::Edit => match event {
                    Event::Input(InputEvent::Key(KeyEvent {
                        code: KeyCode::Esc,
                        ..
                    })) => {
                        input.clear();
                        mode = TuiMode::Progress;
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
                        self.set_limit(new_rate);
                        mode = TuiMode::Progress;
                    },
                    _ => {},
                },
            }
            if self.shutdown.active() {
                break;
            }
            let progress_view = ProgressView {
                start_time,
                progress: self.progress.get(),
            };
            let config = self.config.clone();
            let paused = self.paused.active();
            self.terminal.draw(|f| Self::draw(
                    f,
                    mode,
                    config,
                    paused,
                    progress_view,
                    &input,
            ))?;
        }
        Ok(Cleanup())
    }

    fn toggle_paused(&mut self) {
        self.paused.toggle();
    }

    fn set_limit(&mut self, limit: Option<NonZeroU32>) {
        self.config = Config {
            limit: limit.into(),
            ..self.config
        };
        self.config_tx.send(self.config.clone());
    }

    fn increase_rate(&mut self) {
        let limit = checked_add(self.config.limit(), 10);
        self.set_limit(limit);
    }

    fn decrease_rate(&mut self) {
        let limit = checked_sub(self.config.limit(), 10);
        self.set_limit(limit);
    }

    fn cycle_unit(&mut self) {
        self.config.unit.cycle();
        self.config_tx.send(self.config.clone());
    }

    fn draw<B: Backend>(
        f: &mut Frame<B>,
        mode: TuiMode,
        config: Config,
        paused: bool,
        progress: ProgressView,
        input: &str,
    ) {
        match mode {
            TuiMode::Progress => Self::draw_progress_mode(f, config, paused, progress),
            TuiMode::Edit => Self::draw_update_mode(f, &input),
        }
    }

    fn abbreviate(unit: Unit) -> &'static str {
        match unit {
            Unit::Byte => "B",
            Unit::Line => "L",
            Unit::Null => "#",
        }
    }

    fn draw_progress_mode<B: Backend>(
        f: &mut Frame<B>,
        config: Config,
        paused: bool,
        progress: ProgressView
    ) {

        let pause = if paused { "[PAUSED]" } else { "" };
        let limit = config.limit();
        let limit = limit
            .map(|n| n.get() as u64)
            .map(SizeFormatterBinary::new);
        let ProgressView {
            progress: TransferProgress {
                bytes_transferred,
                ..
            },
            ..
        } = progress;

        let para = if let Some(limit) = limit {
            let unit_abbreviation = Self::abbreviate(config.unit);
            format!(
                "{:.2}B {} [{:.2}{unit}/s] ({:?})",
                SizeFormatterBinary::new(bytes_transferred as u64),
                format_duration(progress.elapsed()),
                limit,
                progress.progress,
                unit=unit_abbreviation,
            )
        } else {
            format!(
                "{:.2}B {} ({:?})",
                SizeFormatterBinary::new(bytes_transferred as u64),
                format_duration(progress.elapsed()),
                progress.progress,
            )
        };

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

struct ProgressView {
    start_time: Instant,
    progress: TransferProgress,
}

impl ProgressView {
    fn elapsed(&self) -> Duration {
        self.start_time.elapsed()
    }
}
