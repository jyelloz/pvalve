use std::{
    io::{
        Result,
        Write,
    },
    num::NonZeroU32,
    time::SystemTime,
    thread::sleep,
};
use governor::{
    Quota,
    RateLimiter,
    clock::SystemClock,
    state::{
        InMemoryState,
        NotKeyed,
    },
};

const LF: u8 = 0x0A;

pub struct RateLimitedWriter<W> {
    inner: W,
    bucket: RateLimiter<NotKeyed, InMemoryState, SystemClock>,
}

impl <W> RateLimitedWriter<W> {
    pub fn new_with_rate(writer: W, rate: NonZeroU32) -> Self {
        Self {
            inner: writer,
            bucket: RateLimiter::direct_with_clock(
                Quota::per_second(rate),
                &SystemClock::default(),
            ),
        }
    }

    pub fn set_rate(&mut self, rate: NonZeroU32) {
        self.bucket = RateLimiter::direct_with_clock(
            Quota::per_second(rate),
            &SystemClock::default(),
        );
    }
}

impl <W> Write for RateLimitedWriter<W> where W: Write {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {

        let newline_position = find_next_newline(buf);

        if let Some(end) = newline_position {
            wait_for_line(&self.bucket);
            self.inner.write(&buf[..end + 1])
        } else {
            self.inner.write(buf)
        }

    }

    fn flush(&mut self) -> Result<()> {
        self.inner.flush()
    }
}

fn find_next_newline(buf: &[u8]) -> Option<usize> {
    buf.iter().position(|b| LF == *b)
}

fn wait_for_line(limiter: &RateLimiter<NotKeyed, InMemoryState, SystemClock>) {
    while let Err(not_until) = limiter.check() {
        let delay = not_until.wait_time_from(SystemTime::now());
        sleep(delay);
    }
}
