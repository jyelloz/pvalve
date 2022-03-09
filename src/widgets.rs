use std::time::Duration;
use std::num::NonZeroU32;

use tui::{
    Frame,
    backend::Backend,
    buffer::Buffer,
    layout::{
        Rect,
        Layout,
        Constraint,
        Direction,
    },
    style::{
        Style,
        Color,
        Modifier,
    },
    widgets::{
        Widget,
        Paragraph,
    },
};

use crossterm::event::{
    Event,
    KeyEvent,
    KeyCode,
};

use size_format::{
    SizeFormatterBinary,
    SizeFormatterSI,
};

use super::config::Unit;
use super::progress::TransferProgress;

pub trait InteractiveWidget: Sized {
    fn render<B: Backend>(self, frame: &mut Frame<B>);
}

pub trait KeyboardInput {
    type Response;
    fn input(&mut self, event: Event) -> Option<Self::Response>;
}

pub struct ObservedRateView(pub TransferProgress, pub Unit);

impl ObservedRateView {
    fn text_byte(progress: &TransferProgress) -> String {
        format!(
            "[{}B/s]",
            SizeFormatterBinary::new(progress.bytes_transferred as u64)
        )
    }
    fn text_line(progress: &TransferProgress) -> String {
        format!(
            "[{}L/s]",
            SizeFormatterSI::new(progress.lines_transferred as u64)
        )
    }
    fn text_null(progress: &TransferProgress) -> String {
        format!(
            "[{}#/s]",
            SizeFormatterSI::new(progress.nulls_transferred as u64)
        )
    }
}

impl Widget for ObservedRateView {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let ObservedRateView(progress, unit) = self;
        let text = match unit {
            Unit::Byte => Self::text_byte(&progress),
            Unit::Line => Self::text_line(&progress),
            Unit::Null => Self::text_null(&progress),
        };
        let para = Paragraph::new(text);
        para.render(area, buf);
    }
}

pub struct DurationView(Duration);

impl DurationView {
    fn text(&self) -> String {
        let duration = chrono::Duration::from_std(self.0)
            .unwrap();
        format!(
            "{}:{:02}:{:02}",
            duration.num_hours(),
            duration.num_minutes() % 60,
            duration.num_seconds() % 60,
        )
    }
}

impl Widget for DurationView {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let para = Paragraph::new(self.text());
        para.render(area, buf);
    }
}

pub struct EditRateState(String);

impl EditRateState {
    pub fn new() -> Self {
        Self(String::new())
    }
    pub fn borrow(&self) -> &str {
        &self.0
    }
}

pub enum EditRateResponse {
    Cancelled,
    NewRate(NonZeroU32),
}

impl From<NonZeroU32> for EditRateResponse {
    fn from(rate: NonZeroU32) -> Self {
        Self::NewRate(rate)
    }
}

impl Into<Option<NonZeroU32>> for EditRateResponse {
    fn into(self) -> Option<NonZeroU32> {
        if let Self::NewRate(rate) = self {
            Some(rate)
        } else {
            None
        }
    }
}

impl KeyboardInput for EditRateState {
    type Response = EditRateResponse;
    fn input(&mut self, event: Event) -> Option<Self::Response> {
        let Self(input) = self;
        match event {
            Event::Key(KeyEvent {
                code: KeyCode::Esc,
                ..
            }) => {
                input.clear();
                Some(Self::Response::Cancelled)
            },
            Event::Key(KeyEvent {
                code: KeyCode::Char(code @ '0'..='9'),
                ..
            }) => {
                input.push(code);
                None
            },
            Event::Key(KeyEvent {
                code: KeyCode::Backspace,
                ..
            }) => {
                input.pop();
                None
            },
            Event::Key(KeyEvent {
                code: KeyCode::Enter,
                ..
            }) => {
                let rate = u32::from_str_radix(&input, 10)
                    .ok()
                    .and_then(NonZeroU32::new)
                    .map(Self::Response::from);
                input.clear();
                rate
            },
            _ => None,
        }
    }
}

pub struct EditRateView<'a>(pub &'a str);

impl <'a> InteractiveWidget for EditRateView<'a> {
    fn render<B: Backend>(self, frame: &mut Frame<B>) {
        let Self(input) = self;
        let row = Rect {
            height: 1,
            ..frame.size()
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
            frame.set_cursor(r.x + input_length, r.y);
            frame.render_widget(para, l);
            frame.render_widget(input, r);
        }
    }
}
