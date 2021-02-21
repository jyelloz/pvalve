use nonzero_ext::nonzero;
use std::{
    fs::OpenOptions, iter, num::NonZeroU32, sync::mpsc::Sender, time::Duration,
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

use crate::ipc::Message;

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
            _ => panic!("failed to iterate input events"),
        }
    }
}

fn checked_add(value: NonZeroU32, increment: u32) -> NonZeroU32 {
    value
        .get()
        .checked_add(increment)
        .and_then(NonZeroU32::new)
        .unwrap_or(value)
}

fn checked_sub(value: NonZeroU32, increment: u32) -> NonZeroU32 {
    value
        .get()
        .checked_sub(increment)
        .and_then(NonZeroU32::new)
        .unwrap_or(value)
}

pub fn user_interface(tx: Sender<Message>) -> anyhow::Result<()> {
    let mut rate = nonzero!(1u32);
    let mut tty = OpenOptions::new().read(true).write(true).open("/dev/tty")?;
    terminal::enable_raw_mode()?;
    execute!(tty, terminal::EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(tty);
    let mut terminal = Terminal::new(backend)?;
    let events = iter::once(Event::Tick).chain(Events);
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
        terminal.draw(|f| draw(f, rate.clone()))?;
    }
    Ok(())
}

pub fn cleanup() -> anyhow::Result<()> {
    let mut tty = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")?;
    execute!(tty, terminal::LeaveAlternateScreen)?;
    terminal::disable_raw_mode()?;
    Ok(())
}

fn draw<B: Backend>(f: &mut Frame<B>, rate: NonZeroU32) {
    let para = Paragraph::new(format!("copying at {} bytes/sec", rate));

    let size = f.size();

    let row = Rect::new(0, 0, size.width, 1);

    f.render_widget(para, row);
}
