use std::time::{
    Duration,
    Instant,
};
use watch::WatchReceiver;

#[derive(Debug, Default, Clone, Copy, PartialEq, PartialOrd, Ord, Eq)]
pub struct TransferProgress {
    pub bytes_transferred: usize,
    pub lines_transferred: usize,
    pub nulls_transferred: usize,
}

pub struct TransferProgressMonitor(WatchReceiver<TransferProgress>);

impl TransferProgress {
    pub fn add_bytes(&mut self, n: usize) {
        self.bytes_transferred += n;
    }
    pub fn add_lines(&mut self, n: usize) {
        self.lines_transferred += n;
    }
    pub fn add_nulls(&mut self, n: usize) {
        self.nulls_transferred += n;
    }
}

impl std::ops::Add for TransferProgress {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self {
            bytes_transferred: self.bytes_transferred + other.bytes_transferred,
            lines_transferred: self.lines_transferred + other.lines_transferred,
            nulls_transferred: self.nulls_transferred + other.nulls_transferred,
        }
    }
}

impl std::ops::Div<usize> for TransferProgress {
    type Output = Self;
    fn div(mut self, rhs: usize) -> Self::Output {
        self.bytes_transferred /= rhs;
        self.lines_transferred /= rhs;
        self.nulls_transferred /= rhs;
        self
    }
}

impl std::iter::Sum for TransferProgress {
    fn sum<I: Iterator<Item=Self>>(iter: I) -> Self {
        iter.fold(Self::default(), |a, b| a + b)
    }
}

impl TransferProgressMonitor {
    pub fn new(rx: WatchReceiver<TransferProgress>) -> Self {
        Self(rx)
    }
    pub fn get(&mut self) -> TransferProgress {
        self.0.get()
    }
}

#[derive(Clone, Copy)]
pub struct CumulativeTransferProgress {
    pub start_time: Instant,
    pub progress: TransferProgress,
}

impl CumulativeTransferProgress {
    pub fn elapsed(&self) -> Duration {
        self.start_time.elapsed()
    }
}
