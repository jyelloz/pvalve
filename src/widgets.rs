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

use super::unit::Unit;
use super::progress::{
    CumulativeTransferProgress,
    TransferProgress,
};

pub trait InteractiveWidget: Sized {
    fn render<B: Backend>(self, frame: &mut Frame<B>);
}

pub trait KeyboardInput {
    type Response;
    fn input(&mut self, event: Event) -> Option<Self::Response>;
}

pub struct ObservedRateView(pub TransferProgress, pub Unit, Option<NonZeroU32>);

impl ObservedRateView {
    const RELATIVE_TOLERANCE: f32 = 0.1f32;
    const ABSOLUTE_TOLERANCE: usize = 1;
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
    fn scalar_progress(&self) -> usize {
        let Self(progress, unit, ..) = self;
        match *unit {
            Unit::Byte => progress.bytes_transferred,
            Unit::Line => progress.lines_transferred,
            Unit::Null => progress.nulls_transferred,
        }
    }
    fn distance_from_limit(&self) -> Option<(bool, usize, f32)> {
        let Self(_, _, limit) = self;
        if let Some(limit) = limit.map(NonZeroU32::get) {
            let scalar_progress = self.scalar_progress();
            let exceeded = scalar_progress >= limit as usize;
            let distance = ((limit as isize) - (scalar_progress as isize)).abs();
            let relative = ((distance as f32) / (limit as f32)).abs();
            Some((exceeded, distance as usize, relative))
        } else {
            None
        }
    }
    fn saturated(&self) -> bool {
        if let Some((exceeded, absolute, relative)) = self.distance_from_limit() {
            exceeded
            ||
            absolute <= Self::ABSOLUTE_TOLERANCE
            ||
            relative <= Self::RELATIVE_TOLERANCE
        } else {
            false
        }
    }

    pub fn as_text(&self) -> String {
        let ObservedRateView(progress, unit, ..) = self;
        match unit {
            Unit::Byte => Self::text_byte(&progress),
            Unit::Line => Self::text_line(&progress),
            Unit::Null => Self::text_null(&progress),
        }
    }
}

impl Widget for ObservedRateView {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let saturated = self.saturated();
        let text = self.as_text();
        let style = if saturated {
            Style::default().add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        let para = Paragraph::new(text).style(style);
        para.render(area, buf);
    }
}

pub struct DurationView(Duration);

impl DurationView {
    fn text(&self) -> String {
        let Self(duration) = self;
        format_duration(duration)
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

fn abbreviate(unit: Unit) -> &'static str {
    match unit {
        Unit::Byte => "B",
        Unit::Line => "L",
        Unit::Null => "#",
    }
}

fn format_duration(duration: &Duration) -> String {
    let secs = duration.as_secs();
    let hours = secs / 3600;
    let minutes = (secs / 60) % 60;
    let seconds = secs % 60;
    format!("{}:{:02}:{:02}", hours, minutes, seconds)
}

#[derive(Clone, Copy)]
struct AbsoluteTransferProgress(CumulativeTransferProgress, Unit);

impl std::fmt::Display for AbsoluteTransferProgress {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Self(progress, unit) = *self;
        let bytes_transferred = SizeFormatterBinary::new(
            progress.progress.bytes_transferred as u64
        );
        let unit_abbreviation = abbreviate(unit);
        let duration = format_duration(&progress.elapsed());
        let CumulativeTransferProgress { progress, .. } = progress;
        match unit {
            Unit::Byte => write!(
                fmt,
                "{:.2}{unit} {}",
                bytes_transferred,
                duration,
                unit=unit_abbreviation,
            ),
            Unit::Line => write!(
                fmt,
                "{:.2}{unit} ({}B) {}",
                SizeFormatterSI::new(progress.lines_transferred as u64),
                bytes_transferred,
                duration,
                unit=unit_abbreviation,
            ),
            Unit::Null => write!(
                fmt,
                "{:.2}{unit} ({}B) {}",
                SizeFormatterSI::new(progress.nulls_transferred as u64),
                bytes_transferred,
                duration,
                unit=unit_abbreviation,
            ),
        }
    }
}

pub struct TransferProgressView {
    pub paused: bool,
    pub limit: Option<NonZeroU32>,
    pub unit: Unit,
    pub cumulative: CumulativeTransferProgress,
    pub instantaneous: TransferProgress,
}

impl InteractiveWidget for TransferProgressView {
    fn render<B: Backend>(self, frame: &mut Frame<B>) {
        let Self { paused, limit, unit, cumulative, instantaneous } = self;
        let pause = if paused { "[PAUSED]" } else { "" };

        let para = format!("{}", AbsoluteTransferProgress(cumulative, unit));

        let row = Rect {
            height: 1,
            ..frame.size()
        };

        let layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(para.len() as u16),
                Constraint::Max(1),
                Constraint::Length(10),
                Constraint::Length(pause.len() as u16),
            ])
            .split(row);

        let progress = Paragraph::new(para);
        let speed = ObservedRateView(instantaneous, unit, limit);
        let pause = Paragraph::new(pause)
            .style(Style::default().add_modifier(Modifier::RAPID_BLINK));

        if let [l, pad, c, r] = *layout {
            frame.render_widget(progress, l);
            frame.render_widget(Paragraph::new(" "), pad);
            frame.render_widget(speed, c);
            frame.render_widget(pause, r);
        }
    }
}
