use governor::{
    clock::{Clock as _, DefaultClock},
    state::{InMemoryState, NotKeyed},
    NegativeMultiDecision, Quota, RateLimiter,
};
use std::{
    io::{Error, ErrorKind, Result, Write},
    num::NonZeroU32,
    sync::mpsc::Receiver,
    thread::sleep,
    time::{Duration, Instant},
};

use nonzero_ext::nonzero;

use crate::ipc::{Message, ProgressMessage};
use crate::memslot::WriteHalf;

const NUL: u8 = 0x0;
const LF: u8 = 0xA;

type DirectRateLimiter<C> = RateLimiter<NotKeyed, InMemoryState, C>;

fn find_newline(buf: &[u8], delimiter: u8) -> Option<usize> {
    buf.iter().position(|byte| delimiter == *byte)
}

struct Progress {
    bytes_transferred: usize,
    time_started: Instant,
}

impl Progress {
    fn duration(&self) -> Duration {
        Instant::now().duration_since(self.time_started)
    }
    fn add(&mut self, bytes: usize) -> usize {
        self.bytes_transferred += bytes;
        self.bytes_transferred
    }
}

impl Default for Progress {
    fn default() -> Self {
        Self {
            bytes_transferred: 0,
            time_started: Instant::now(),
        }
    }
}

pub struct RateLimitedWriter<W> {
    inner: W,
    limiter: DirectRateLimiter<DefaultClock>,
    rate: NonZeroU32,
    rx: Option<Receiver<Message>>,
    tx: Option<WriteHalf<ProgressMessage>>,
    progress: Progress,
}

impl<W> RateLimitedWriter<W> {
    pub fn writer_with_rate(writer: W, rate: NonZeroU32) -> Self {
        Self {
            inner: writer,
            limiter: RateLimiter::direct(Quota::per_second(rate)),
            rate,
            rx: None,
            tx: None,
            progress: Default::default(),
        }
    }

    pub fn writer_with_rate_and_updates(
        writer: W,
        rate: NonZeroU32,
        rate_updates: Receiver<Message>,
        progress_updates: WriteHalf<ProgressMessage>,
    ) -> Self {
        Self {
            inner: writer,
            limiter: RateLimiter::direct(Quota::per_second(rate)),
            rate,
            rx: Some(rate_updates),
            tx: Some(progress_updates),
            progress: Default::default(),
        }
    }

    pub fn get_rate(&self) -> NonZeroU32 {
        self.rate
    }

    fn set_rate(&mut self, rate: NonZeroU32) {
        self.limiter = RateLimiter::direct(
            Quota::per_second(rate).allow_burst(nonzero!(1u32))
        );
        self.rate = rate;
    }

    fn update(&mut self) -> bool {
        if let Some(updates) = &mut self.rx {
            if let Ok(message) = updates.try_recv() {
                match message {
                    Message::UpdateRate(rate) => {
                        self.set_rate(rate);
                        return false;
                    }
                    Message::Interrupted => {
                        return true;
                    }
                }
            }
        }
        return false;
    }

    fn update_progress(&mut self, bytes_transferred: usize) {
        if let Some(tx) = &mut self.tx {
            let bytes_transferred = self.progress.add(bytes_transferred);
            let duration = self.progress.duration();
            tx.set(ProgressMessage::Transfer(bytes_transferred, duration));
        }
    }
}

impl<W> Write for RateLimitedWriter<W>
where
    W: Write,
{
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        if self.update() {
            return Err(Error::new(
                ErrorKind::BrokenPipe,
                "Process interrupted",
            ));
        }
        let len = buf.len();
        let end = wait_for_bytes(&self.limiter, len as u32) as usize;
        let bytes_transferred = self.inner.write(&buf[..end])?;
        if bytes_transferred < len {
            self.flush()?;
        }
        self.update_progress(bytes_transferred);
        Ok(bytes_transferred)
    }

    fn flush(&mut self) -> Result<()> {
        self.inner.flush()
    }
}

pub struct RateLimitedLineWriter<W> {
    inner: W,
    limiter: DirectRateLimiter<DefaultClock>,
    delimiter: u8,
}

impl<W> RateLimitedLineWriter<W> {
    pub fn new_linefeed_separated(inner: W, rate: NonZeroU32) -> Self {
        Self {
            inner,
            limiter: RateLimiter::direct(Quota::per_second(rate)),
            delimiter: LF,
        }
    }

    pub fn new_null_separated(inner: W, rate: NonZeroU32) -> Self {
        Self {
            inner,
            limiter: RateLimiter::direct(Quota::per_second(rate)),
            delimiter: NUL,
        }
    }

    pub fn set_rate(&mut self, rate: NonZeroU32) {
        self.limiter = RateLimiter::direct(Quota::per_second(rate));
    }
}

impl<W> Write for RateLimitedLineWriter<W>
where
    W: Write,
{
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        if let Some(end) = find_newline(buf, self.delimiter) {
            wait_for_line(&self.limiter);
            self.inner.write(&buf[..end + 1])
        } else {
            self.inner.write(buf)
        }
    }

    fn flush(&mut self) -> Result<()> {
        self.inner.flush()
    }
}

fn wait_for_bytes(limiter: &DirectRateLimiter<DefaultClock>, goal: u32) -> u32 {
    if goal <= 2 {
        let clock = DefaultClock::default();
        let now = clock.now();
        if let Err(not_until) = limiter.check() {
            let delay = not_until.wait_time_from(now);
            sleep(delay);
            return wait_for_bytes(limiter, 1);
        } else {
            return 1;
        }
    }

    let goal_value = NonZeroU32::new(goal);

    if goal_value.is_none() {
        return 0;
    }

    match limiter.check_n(goal_value.unwrap()) {
        Ok(_) => goal,
        Err(NegativeMultiDecision::InsufficientCapacity(part)) => {
            wait_for_bytes(limiter, part)
        }
        Err(NegativeMultiDecision::BatchNonConforming(_, _)) => {
            wait_for_bytes(limiter, goal / 2)
        }
    }
}

fn wait_for_line(limiter: &DirectRateLimiter<DefaultClock>) {
    let clock = DefaultClock::default();
    let now = clock.now();
    while let Err(not_until) = limiter.check() {
        let delay = not_until.wait_time_from(now);
        sleep(delay);
    }
}
