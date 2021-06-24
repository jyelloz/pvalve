use std::{
    io::{
        Error,
        ErrorKind,
        Result,
        Write,
    },
    num::NonZeroU32,
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
    RateLimiter as GovernorRateLimiter,
};

use watch::{
    channel,
    WatchSender,
};

use crate::{
    progress::{
        TransferProgress,
        TransferProgressMonitor,
    },
    config::{
        ConfigMonitor,
        LatchMonitor,
        Unit,
    },
};

const NUL: u8 = 0x0;
const LF: u8 = 0xA;

pub trait WriteExt<W> {
    /// Wrap any writer into one which can report progress.
    fn progress(self) -> ProgressWriter<W>;
    /// Wrap any writer into one which can be paused and resumed.
    fn pauseable(self, paused: LatchMonitor) -> PauseableWriter<W>;
    /// Wrap any writer into one which can be cancelled.
    fn cancellable(self, cancelled: LatchMonitor) -> CancellableWriter<W>;
    /// Wrap any writer into one with a throughput limit.
    fn limited(self, config: ConfigMonitor) -> RateLimitedWriter<W, DynamicRateLimiter>;
}

impl <W: Write> WriteExt<W> for W {
    fn progress(self) -> ProgressWriter<W> {
        ProgressWriter::new(self)
    }
    fn pauseable(self, paused: LatchMonitor) -> PauseableWriter<W> {
        PauseableWriter {
            inner: self,
            paused,
        }
    }
    fn cancellable(self, cancelled: LatchMonitor) -> CancellableWriter<W> {
        CancellableWriter {
            inner: self,
            cancelled,
        }
    }
    fn limited(self, config: ConfigMonitor) -> RateLimitedWriter<W, DynamicRateLimiter> {
        RateLimitedWriter::writer_with_config(self, config)
    }
}

#[derive(Clone)]
pub struct ProgressWriter<W> {
    inner: W,
    transfer_progress: TransferProgress,
    tx: WatchSender<TransferProgress>,
}

impl <W> ProgressWriter<W> {
    fn new(inner: W) -> Self {
        let transfer_progress = TransferProgress::default();
        let (tx, _) = channel(transfer_progress);
        Self {
            inner,
            transfer_progress,
            tx,
        }
    }
    pub fn transfer_progress(&mut self) -> TransferProgressMonitor {
        TransferProgressMonitor::new(self.tx.subscribe())
    }
}

impl <W: Write> Write for ProgressWriter<W> {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        let bytes_transferred = self.inner.write(buf)?;
        let slice = &buf[..bytes_transferred];
        self.transfer_progress.add_bytes(bytes_transferred);
        self.transfer_progress.add_lines(annotate_lines(slice).len());
        self.transfer_progress.add_nulls(annotate_nulls(slice).len());
        self.tx.send(self.transfer_progress);
        Ok(bytes_transferred)
    }
    fn flush(&mut self) -> Result<()> {
        self.inner.flush()
    }
}

type DirectRateLimiter<C> = GovernorRateLimiter<NotKeyed, InMemoryState, C>;

pub struct RateLimitedWriter<W, R> {
    inner: W,
    config: ConfigMonitor,
    rate_limiter: R,
}

pub trait RateLimiter {
    /// Request the desired amount of tokens.
    ///
    /// Returns the amount of tokens granted, which is always less than or equal
    /// to what was requested.
    ///
    /// If none are available at the time of request, it blocks until there is
    /// at least one token available and acquires whatever portion of the
    /// requested amount that it can.
    fn request(&mut self, tokens: u32) -> u32;
}

pub struct DynamicRateLimiter {
    limiter: Option<DirectRateLimiter<DefaultClock>>,
}

impl DynamicRateLimiter {
    pub fn new(limit: Option<NonZeroU32>) -> Self {
        Self {
            limiter: Self::limiter(limit)
        }
    }
    fn swapout(&mut self, limit: Option<NonZeroU32>) {
        self.limiter = Self::limiter(limit);
    }
    fn limiter(
        limit: Option<NonZeroU32>
    ) -> Option<DirectRateLimiter<DefaultClock>> {
        limit.map(Quota::per_second)
            .map(DirectRateLimiter::direct)
    }
}

impl RateLimiter for DynamicRateLimiter {
    fn request(&mut self, tokens: u32) -> u32 {
        if tokens < 1 {
            return 0;
        }
        if let Some(limiter) = &mut self.limiter {
            wait_for_at_most(limiter, tokens)
        } else {
            tokens
        }
    }
}

impl <W> RateLimitedWriter<W, DynamicRateLimiter> {

    pub fn writer_with_config(writer: W, mut config: ConfigMonitor) -> Self {
        let rate_limiter = DynamicRateLimiter::new(config.limit().into());
        Self {
            inner: writer,
            rate_limiter,
            config,
        }
    }

    fn get_largest_slice<'a>(&mut self, buf: &'a [u8]) -> &'a [u8] {
        let points = self.annotate(buf);
        let buffer_cost = points.len().min(u32::MAX as usize) as u32;
        let end = if buffer_cost < 1 {
            buf.len()
        } else {
            let allowable_tokens = self.rate_limiter.request(buffer_cost);
            points[allowable_tokens as usize - 1] + 1
        };
        &buf[..end]
    }

    fn annotate(&mut self, buf: &[u8]) -> Vec<usize> {
        match self.config.unit() {
            Unit::Byte => annotate_bytes(buf),
            Unit::Line => annotate_lines(buf),
            Unit::Null => annotate_nulls(buf),
        }
    }

    fn set_rate(&mut self, rate: NonZeroU32) {
        self.rate_limiter.swapout(rate.into());
    }

    fn poll_for_config_update(&mut self) {
        if let Some(new_rate) = self.config.limit_if_new() {
            self.set_rate(new_rate);
        }
    }
}

impl <W: Write> Write for RateLimitedWriter<W, DynamicRateLimiter> {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        self.poll_for_config_update();
        let slice = self.get_largest_slice(buf);
        let bytes_transferred = self.inner.write(slice)?;
        if bytes_transferred < buf.len() {
            self.flush()?;
        }
        Ok(bytes_transferred)
    }

    fn flush(&mut self) -> Result<()> {
        self.inner.flush()
    }
}

fn annotate_bytes(buf: &[u8]) -> Vec<usize> {
    buf.iter()
        .enumerate()
        .map(|(i, _)| i)
        .collect()
}

fn annotate_lines(buf: &[u8]) -> Vec<usize> {
    buf.iter()
        .enumerate()
        .filter(|(_, b)| LF == **b)
        .map(|(i, _)| i)
        .collect()
}

fn annotate_nulls(buf: &[u8]) -> Vec<usize> {
    buf.iter()
        .enumerate()
        .filter(|(_, b)| NUL == **b)
        .map(|(i, _)| i)
        .collect()
}

/// Should never take more than ~32 recursive steps to terminate.
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
