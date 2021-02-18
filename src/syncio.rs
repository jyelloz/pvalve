use std::{
    io::{
        Result,
        Write,
    },
    num::NonZeroU32,
    thread::sleep,
};
use governor::{
    Quota,
    RateLimiter,
    clock::{
        Clock as _,
        DefaultClock,
    },
    NegativeMultiDecision,
    state::{
        InMemoryState,
        NotKeyed,
    },
};

const LF: u8 = 0x0A;

type DirectRateLimiter<C> = RateLimiter<NotKeyed, InMemoryState, C>;

fn find_newline(buf: &[u8]) -> Option<usize> {
    buf.iter().position(|byte| LF == *byte)
}

pub struct RateLimitedWriter<W> {
    inner: W,
    limiter: DirectRateLimiter<DefaultClock>,
}

pub struct RateLimitedLineWriter<W> {
    inner: W,
    limiter: DirectRateLimiter<DefaultClock>,
}

impl <W> RateLimitedWriter<W> {
    pub fn writer_with_rate(writer: W, rate: NonZeroU32) -> Self {
        Self {
            inner: writer,
            limiter: RateLimiter::direct(Quota::per_second(rate)),
        }
    }

    pub fn set_rate(&mut self, rate: NonZeroU32) {
        self.limiter = RateLimiter::direct(Quota::per_second(rate));
    }
}

impl <W> Write for RateLimitedWriter<W> where W: Write {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        let len = buf.len();
        let end = wait_for_bytes(
            &self.limiter,
            len as u32,
        ) as usize;
        let bytes_written = self.inner.write(&buf[..end])?;
        if end < len {
            self.flush()?;
        }
        Ok(bytes_written)
    }

    fn flush(&mut self) -> Result<()> {
        self.inner.flush()
    }
}

impl <W> Write for RateLimitedLineWriter<W> where W: Write {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        let newline_position = find_newline(buf);
        if let Some(end) = newline_position {
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

fn wait_for_bytes(
    limiter: &DirectRateLimiter<DefaultClock>,
    goal: u32,
) -> u32 {

    if goal <= 2 {
        let clock = DefaultClock::default();
        let now = clock.now();
        if let Err(not_until) = limiter.check() {
            let delay = not_until.wait_time_from(now);
            eprintln!("waiting {:?} for {} bytes", delay, goal);
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
        },
        Err(NegativeMultiDecision::BatchNonConforming(_, _)) => {
            wait_for_bytes(limiter, goal / 2)
        },
    }
}

fn wait_for_line(limiter: &DirectRateLimiter<DefaultClock>) {
    let clock = DefaultClock::default();
    while let Err(not_until) = limiter.check() {
        let delay = not_until.wait_time_from(clock.now());
        sleep(delay);
    }
}
