use std::{
    num::NonZeroU32,
    time::Duration,
    thread::sleep,
};

use governor::{
    clock::{
        Clock,
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

#[derive(Debug, Clone, Copy, PartialOrd, Ord, PartialEq, Eq)]
enum Unit {
    Byte,
    Line,
    Null,
}

type Governor<C> = RateLimiter<NotKeyed, InMemoryState, C>;

struct Limiter<C: Clock> {
    unit: Unit,
    limit: Option<NonZeroU32>,
    governor: Governor<C>,
}

impl <C: Clock> Limiter<C> {

    fn limit(&mut self, buffer: &[u8]) -> usize {
        match self.limit {
            Some(limit) => match self.unit {
                Unit::Byte => self.limit_bytes(buffer, limit),
                Unit::Line => self.limit_lines(buffer, limit),
                Unit::Null => self.limit_nulls(buffer, limit),
            },
            _ => buffer.len(),
        }
    }

    fn limit_bytes(&mut self, buffer: &[u8], limit: NonZeroU32) -> usize {
        0
    }

    fn limit_lines(&mut self, buffer: &[u8], limit: NonZeroU32) -> usize {
        0
    }

    fn limit_nulls(&mut self, buffer: &[u8], limit: NonZeroU32) -> usize {
        0
    }

    pub fn set_limit(&mut self, limit: u32) {
        self.limit = NonZeroU32::new(limit);
    }

    fn acquire_maximum_available(&mut self, request: NonZeroU32) -> u32 {

        if let Ok(_) = self.governor.check_n(request) {
            return request.get();
        }

        let request = request.get();

        if request < 2 {
            return self.acquire_one();
        }

        let request = NonZeroU32::new(request >> 1)
            .expect("number must be greater than zero");

        self.acquire_maximum_available(request)

    }

    fn acquire_one(&mut self) -> u32 {
        while let Err(_) = self.governor.check() {
            sleep(Duration::from_millis(100));
        }
        1
    }

}

fn count_bytes(buffer: &[u8]) -> usize {
    buffer.len()
}

fn count_lines(buffer: &[u8]) -> usize {
    buffer.iter()
        .filter(|byte| **byte == 0x0Au8)
        .count()
}

fn count_nulls(buffer: &[u8]) -> usize {
    buffer.iter()
        .filter(|byte| **byte == 0x00u8)
        .count()
}
