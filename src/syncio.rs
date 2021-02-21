use std::{
    io::{
        Result,
        Write,
        Error,
        ErrorKind,
    },
    num::NonZeroU32,
    thread::sleep,
    sync::mpsc::Receiver,
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

use crate::ipc::Message;

const NUL: u8 = 0x0;
const LF: u8 = 0xA;

type DirectRateLimiter<C> = RateLimiter<NotKeyed, InMemoryState, C>;

fn find_newline(buf: &[u8], delimiter: u8) -> Option<usize> {
    buf.iter().position(|byte| delimiter == *byte)
}

pub struct RateLimitedWriter<W> {
    inner: W,
    limiter: DirectRateLimiter<DefaultClock>,
    rate: NonZeroU32,
    updates: Option<Receiver<Message>>,
}

impl <W> RateLimitedWriter<W> {
    pub fn writer_with_rate(writer: W, rate: NonZeroU32) -> Self {
        Self {
            inner: writer,
            limiter: RateLimiter::direct(Quota::per_second(rate)),
            rate,
            updates: None,
        }
    }

    pub fn writer_with_rate_and_updates(
        writer: W,
        rate: NonZeroU32,
        rate_updates: Receiver<Message>,
    ) -> Self {
        Self {
            inner: writer,
            limiter: RateLimiter::direct(Quota::per_second(rate)),
            rate,
            updates: Some(rate_updates),
        }
    }

    pub fn get_rate(&self) -> NonZeroU32 {
        self.rate
    }

    fn set_rate(&mut self, rate: NonZeroU32) {
        self.limiter = RateLimiter::direct(Quota::per_second(rate));
        self.rate = rate;
    }

    fn update(&mut self) ->  bool {
        if let Some(updates) = &mut self.updates {
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

}

impl <W> Write for RateLimitedWriter<W> where W: Write {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        if self.update() {
            return Err(Error::new(ErrorKind::BrokenPipe, "Process interrupted"));
        }
        let len = buf.len();
        let end = wait_for_bytes(
            &self.limiter,
            len as u32,
        ) as usize;
        let bytes_written = self.inner.write(&buf[..end])?;
        if bytes_written < len {
            self.flush()?;
        }
        Ok(bytes_written)
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

impl <W> RateLimitedLineWriter<W> {

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

impl <W> Write for RateLimitedLineWriter<W> where W: Write {
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

fn wait_for_bytes(
    limiter: &DirectRateLimiter<DefaultClock>,
    goal: u32,
) -> u32 {

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
        },
        Err(NegativeMultiDecision::BatchNonConforming(_, _)) => {
            wait_for_bytes(limiter, goal / 2)
        },
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
