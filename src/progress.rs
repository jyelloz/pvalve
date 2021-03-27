use std::{
    sync::{
        Arc,
        RwLock,
        atomic::{
            AtomicUsize,
            Ordering,
        },
    },
    time::{Duration, Instant},
};
use sum_queue::{QueueStats, SumQueue};

use lazy_static::lazy_static;

lazy_static! {
    static ref CURRENT: Arc<RwLock<ProgressView>> = Arc::new(RwLock::new(
        Default::default()
    ));
}

pub struct Sample {
    pub timestamp: Instant,
    pub bytes_transferred: usize,
}

pub struct Progress {
    pub start_time: Instant,
    pub bytes_transferred: usize,
    recent: SumQueue<usize>,
    stats: Option<QueueStats<usize>>,
}

#[derive(Clone, Debug)]
pub struct ProgressView {
    pub start_time: Instant,
    pub bytes_transferred: usize,
    pub recent_throughput: f32,
}

pub struct ProgressCounter(Arc<AtomicUsize>);

impl ProgressView {
    pub fn elapsed(&self) -> Duration {
        self.start_time.elapsed()
    }
    pub fn average_throughput(&self) -> f32 {
        self.bytes_transferred as f32 / self.elapsed().as_secs_f32()
    }
}

impl Default for ProgressView {
    fn default() -> Self {
        let now = Instant::now();
        Self {
            start_time: now,
            bytes_transferred: 0,
            recent_throughput: 0f32,
        }
    }
}

impl Progress {
    pub fn with_window(start_time: Instant, window: Duration) -> Self {
        Self {
            bytes_transferred: 0,
            start_time,
            recent: SumQueue::new(window.as_secs()),
            stats: None,
        }
    }

    pub fn update(&mut self, sample: Sample) {
        let bytes_transferred = sample.bytes_transferred;

        self.bytes_transferred += bytes_transferred;

        let stats = self.recent.push_and_stats(bytes_transferred);

        self.stats.replace(stats);
    }

    fn window_transfer_rate(&self) -> f32 {
        if let Some(stats) = &self.stats {
            (stats.sum.unwrap_or(0) as f32) / (self.recent.max_age() as f32)
        } else {
            f32::NAN
        }
    }

    pub fn view(&self) -> ProgressView {
        self.into()
    }
}

impl Default for Progress {
    fn default() -> Self {
        Self::with_window(Instant::now(), Duration::from_secs(5))
    }
}

impl Into<ProgressView> for &Progress {
    fn into(self) -> ProgressView {
        ProgressView {
            start_time: self.start_time,
            bytes_transferred: self.bytes_transferred,
            recent_throughput: self.window_transfer_rate(),
        }
    }
}

impl ProgressCounter {
    pub fn new(counter: Arc<AtomicUsize>) -> Self {
        Self(counter)
    }
    pub fn get(&self) -> usize {
        self.0.load(Ordering::Relaxed)
    }
}

impl Into<usize> for ProgressCounter {
    fn into(self) -> usize {
        self.get()
    }
}
