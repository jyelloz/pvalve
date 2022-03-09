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
    Frame,
    Terminal,
};

use crossterm::{
    event::{poll, read, Event as InputEvent, KeyCode, KeyEvent, KeyModifiers},
    execute, terminal,
};

use thiserror::Error;

use watch::WatchSender;

use super::{
    config::{Config, Latch, LatchMonitor},
    progress::{
        TransferProgress,
        TransferProgressMonitor,
        CumulativeTransferProgress,
    },
    widgets::{
        InteractiveWidget as _,
        KeyboardInput as _,
        EditRateView,
        EditRateState,
        TransferProgressView,
    },
};

#[derive(Debug, Error)]
pub enum UserInterfaceError {
    #[error("I/O error talking to terminal")]
    IO(#[from] io::Error),
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
    cumulative: TransferProgressMonitor,
    instantaneous: TransferProgressMonitor,
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
        cumulative: TransferProgressMonitor,
        instantaneous: TransferProgressMonitor,
        config_tx: WatchSender<Config>,
    ) -> Result<Self> {
        let backend = Self::initialize_backend()?;
        let terminal = Terminal::new(backend)?;
        Ok(Self {
            terminal,
            shutdown,
            config,
            config_tx,
            paused,
            aborted,
            cumulative,
            instantaneous,
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
        let mut rate = EditRateState::new();
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
                        code: KeyCode::Char('`'),
                        ..
                    })) => { self.toggle_speed_limit(); },
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
                TuiMode::Edit => if let Event::Input(event) = event {
                    if let Some(rate) = rate.input(event) {
                        let rate: Option<NonZeroU32> = rate.into();
                        self.set_limit(rate);
                        mode = TuiMode::Progress;
                    }
                },
            }
            if self.shutdown.active() {
                break;
            }
            let cumulative_progress = CumulativeTransferProgress {
                start_time,
                progress: self.cumulative.get(),
            };
            let config = self.config;
            let paused = self.paused.active();
            let speed = self.instantaneous.get();
            self.terminal.draw(|f| Self::draw(
                    f,
                    mode,
                    config,
                    paused,
                    cumulative_progress,
                    speed,
                    rate.borrow(),
            ))?;
        }
        Ok(Cleanup())
    }

    fn toggle_paused(&mut self) {
        self.paused.toggle();
    }

    fn toggle_speed_limit(&mut self) {
        self.config.toggle_limit();
        self.config_tx.send(self.config);
    }

    fn set_limit(&mut self, limit: Option<NonZeroU32>) {
        self.config = Config {
            limit: limit.into(),
            ..self.config
        };
        self.config_tx.send(self.config);
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
        self.config_tx.send(self.config);
    }

    fn draw<B: Backend>(
        frame: &mut Frame<B>,
        mode: TuiMode,
        config: Config,
        paused: bool,
        cumulative: CumulativeTransferProgress,
        instantaneous: TransferProgress,
        input: &str,
    ) {
        match mode {
            TuiMode::Progress => TransferProgressView {
                paused,
                unit: config.unit,
                limit: config.limit(),
                cumulative,
                instantaneous,
            }.render(frame),
            TuiMode::Edit => EditRateView(input).render(frame),
        }
    }

}

impl Drop for UserInterface {
    fn drop(&mut self) {
        Cleanup();
    }
}
