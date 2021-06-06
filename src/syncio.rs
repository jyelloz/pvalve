use std::{
    io::{
        Error,
        ErrorKind,
        Result,
        Write,
    },
    num::NonZeroU32,
    sync::{
        Arc,
        atomic::{
            AtomicUsize,
            Ordering,
        },
    },
    thread::sleep,
    time::Duration,
};

use governor::{
    clock::{
        Clock as _,
        DefaultClock,
    },
    state::{
        InMemoryState,
        NotKeyed,
    },
    NegativeMultiDecision,
    Quota,
    RateLimiter,
};

use crate::{
    progress::ProgressCounter,
    config::{
        ConfigMonitor,
        LatchMonitor,
    },
};

const NUL: u8 = 0x0;
const LF: u8 = 0xA;

pub trait WriteExt<W> {
    fn progress(self) -> ProgressWriter<W>;
    fn pauseable(self, paused: LatchMonitor) -> PauseableWriter<W>;
    fn cancellable(self, cancelled: LatchMonitor) -> CancellableWriter<W>;
}

impl <W: Write> WriteExt<W> for W {
    /// Wrap any writer into one which can report progress.
    fn progress(self) -> ProgressWriter<W> {
        ProgressWriter {
            inner: self,
            bytes_transferred: Arc::new(AtomicUsize::new(0)),
        }
    }
    /// Wrap any writer into one which can be paused and resumed.
    fn pauseable(self, paused: LatchMonitor) -> PauseableWriter<W> {
        PauseableWriter {
            inner: self,
            paused,
        }
    }
    /// Wrap any writer into one which can be cancelled.
    fn cancellable(self, cancelled: LatchMonitor) -> CancellableWriter<W> {
        CancellableWriter {
            inner: self,
            cancelled,
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
    config: ConfigMonitor,
}

impl<W> RateLimitedWriter<W> {
    pub fn writer_with_config(writer: W, mut config: ConfigMonitor) -> Self {
        let clock = DefaultClock::default();
        let limiter = RateLimiter::direct_with_clock(
            Quota::per_second(config.limit()),
            &clock
        );
        Self {
            inner: writer,
            clock,
            limiter,
            config,
        }
    }

    fn set_rate(&mut self, rate: NonZeroU32) {
        self.limiter = RateLimiter::direct_with_clock(
            Quota::per_second(rate),
            &self.clock,
        );
    }

    fn poll_for_config_update(&mut self) {
        if let Some(new_rate) = self.config.limit_if_new() {
            self.set_rate(new_rate);
        }
    }
}

impl<W: Write> Write for RateLimitedWriter<W> {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        self.poll_for_config_update();
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

impl<W: Write> Write for RateLimitedLineWriter<W> {
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

pub struct PauseableWriter<W> {
    inner: W,
    paused: LatchMonitor,
}

impl <W> PauseableWriter<W> {
    fn paused(&mut self) -> bool {
        self.paused.active()
    }
}

impl <W: Write> Write for PauseableWriter<W> {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        while self.paused() {
            sleep(Duration::from_millis(500));
        }
        self.inner.write(buf)
    }
    fn flush(&mut self) -> Result<()> {
        self.inner.flush()
    }
}

pub struct CancellableWriter<W> {
    inner: W,
    cancelled: LatchMonitor,
}

impl <W> CancellableWriter<W> {
    fn cancelled(&mut self) -> bool {
        self.cancelled.active()
    }
}

impl <W: Write> Write for CancellableWriter<W> {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        if self.cancelled() {
            Err(Error::new(ErrorKind::BrokenPipe, "cancelled"))
        } else {
            self.inner.write(buf)
        }
    }

    fn flush(&mut self) -> Result<()> {
        self.inner.flush()
    }
}
