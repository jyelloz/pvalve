use governor::{
    clock::{Clock as _, DefaultClock},
    state::{InMemoryState, NotKeyed},
    NegativeMultiDecision, Quota, RateLimiter,
};
use std::{
    io::{Error, ErrorKind, Result, Write},
    num::NonZeroU32,
    sync::{
        Arc,
        atomic::{
            AtomicBool,
            AtomicUsize,
            Ordering,
        },
        mpsc::Receiver,
    },
    thread::sleep,
    time::Duration,
};

use crate::{
    ipc::Message,
    progress::ProgressCounter,
    config::Config,
};

const NUL: u8 = 0x0;
const LF: u8 = 0xA;

pub trait WithProgress<W> {
    fn progress(self) -> ProgressWriter<W>;
}
impl <W> WithProgress<W> for W where W: Write {
    fn progress(self) -> ProgressWriter<W> {
        ProgressWriter {
            inner: self,
            bytes_transferred: Arc::new(AtomicUsize::new(0)),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ProgressWriter<W> {
    inner: W,
    bytes_transferred: Arc<AtomicUsize>,
}

impl <W> ProgressWriter<W> {
    pub fn bytes_transferred(&self) -> ProgressCounter {
        ProgressCounter::new(self.bytes_transferred.clone())
    }
}

impl <W: Write> Write for ProgressWriter<W> {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        let bytes_transferred = self.inner.write(buf)?;
        self.bytes_transferred.fetch_add(bytes_transferred, Ordering::Relaxed);
        Ok(bytes_transferred)
    }
    fn flush(&mut self) -> Result<()> {
        self.inner.flush()
    }
}

type DirectRateLimiter<C> = RateLimiter<NotKeyed, InMemoryState, C>;

pub struct RateLimitedWriter<W> {
    inner: W,
    limiter: DirectRateLimiter<DefaultClock>,
    clock: DefaultClock,
    rate: NonZeroU32,
    rate_updates: Receiver<Message>,
}

impl<W> RateLimitedWriter<W> {
    pub fn writer_with_rate_and_updates(
        writer: W,
        rate: NonZeroU32,
        rate_updates: Receiver<Message>,
    ) -> Self {
        let clock = DefaultClock::default();
        let limiter = RateLimiter::direct_with_clock(
            Quota::per_second(rate),
            &clock
        );
        Self {
            inner: writer,
            clock,
            limiter,
            rate,
            rate_updates,
        }
    }

    pub fn get_rate(&self) -> NonZeroU32 {
        self.rate
    }

    fn set_rate(&mut self, rate: NonZeroU32) {
        self.limiter = RateLimiter::direct_with_clock(
            Quota::per_second(rate),
            &self.clock,
        );
        self.rate = rate;
    }

    fn handle_control_message(&mut self) -> bool {
        let new_rate = Config::current().limit();
        if new_rate != self.rate {
            self.set_rate(new_rate);
        }
        match self.rate_updates.try_recv() {
            Ok(Message::Interrupted) => {
                true
            },
            _ => false
        }
    }

}

impl<W> Write for RateLimitedWriter<W>
where
    W: Write,
{
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        if self.handle_control_message() {
            return Err(Error::new(
                ErrorKind::BrokenPipe,
                "Process interrupted",
            ));
        }
        let len = buf.len();
        let end = wait_for_at_most(&self.limiter, len as u32) as usize;
        let bytes_transferred = self.inner.write(&buf[..end])?;
        if bytes_transferred < len {
            self.flush()?;
        }
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
            wait_for_one(&self.limiter);
            self.inner.write(&buf[..end + 1])
        } else {
            self.inner.write(buf)
        }
    }

    fn flush(&mut self) -> Result<()> {
        self.inner.flush()
    }
}

fn find_newline(buf: &[u8], delimiter: u8) -> Option<usize> {
    buf.iter().position(|byte| delimiter == *byte)
}

fn wait_for_at_most(limiter: &DirectRateLimiter<DefaultClock>, goal: u32) -> u32 {
    if goal <= 2 {
        let clock = DefaultClock::default();
        let now = clock.now();
        if let Err(not_until) = limiter.check() {
            let delay = not_until.wait_time_from(now);
            sleep(delay);
            wait_for_one(limiter);
        }
        return 1;
    }

    let goal_value = NonZeroU32::new(goal);

    if goal_value.is_none() {
        return 0;
    }

    match limiter.check_n(goal_value.unwrap()) {
        Ok(_) => goal,
        Err(NegativeMultiDecision::InsufficientCapacity(part)) => {
            wait_for_at_most(limiter, part)
        }
        Err(NegativeMultiDecision::BatchNonConforming(_, _)) => {
            wait_for_at_most(limiter, goal / 2)
        }
    }
}

fn wait_for_one(limiter: &DirectRateLimiter<DefaultClock>) {
    let clock = DefaultClock::default();
    let now = clock.now();
    while let Err(not_until) = limiter.check() {
        let delay = not_until.wait_time_from(now);
        sleep(delay);
    }
}

struct Paused(Arc<AtomicBool>);

impl Paused {
    fn new() -> Self {
        Self(Arc::new(AtomicBool::default()))
    }
    fn paused(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

struct PauseableWriter<W> {
    writer: W,
    paused: Paused,
}

impl <W> PauseableWriter<W> {
    fn paused(&self) -> bool {
        self.paused.paused()
    }
}

impl <W: Write> Write for PauseableWriter<W> {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        while self.paused() {
            sleep(Duration::from_millis(500));
        }
        self.write(buf)
    }
    fn flush(&mut self) -> Result<()> {
        self.writer.flush()
    }
}
