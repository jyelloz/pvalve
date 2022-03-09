use std::{
    io::{
        Result,
        Write,
    },
    time::Duration,
};

use watch::{
    channel,
    WatchSender,
};

use sum_queue::{
    SumQueue,
    QueueStats,
};

use super::progress::{
    TransferProgress,
    TransferProgressMonitor,
};

fn mean(data: &mut QueueStats<TransferProgress>, window: Duration) -> TransferProgress {
    let TransferProgress {
        bytes_transferred,
        lines_transferred,
        nulls_transferred,
    } = data.sum.unwrap_or_default();
    let window = window.as_secs_f64();
    let bytes_transferred = bytes_transferred as f64 / window;
    let lines_transferred = lines_transferred as f64 / window;
    let nulls_transferred = nulls_transferred as f64 / window;
    TransferProgress {
        bytes_transferred: bytes_transferred as usize,
        lines_transferred: lines_transferred as usize,
        nulls_transferred: nulls_transferred as usize,
    }
}

pub struct InstantaneousProgressWriter<W> {
    inner: W,
    tx: WatchSender<TransferProgress>,
    q: SumQueue<TransferProgress>,
}

impl <W> InstantaneousProgressWriter<W> {
    pub fn new(inner: W, window: Duration) -> Self {
        let (tx, _) = channel(TransferProgress::default());
        let q = SumQueue::new(window);
        Self {
            inner,
            tx,
            q,
        }
    }
    fn update(&mut self, buf: &[u8]) {
        let sample = TransferProgress {
            bytes_transferred: count_bytes(buf),
            lines_transferred: count_lines(buf),
            nulls_transferred: count_nulls(buf),
        };
        let mut stats = self.q.push_and_stats(sample);
        let mean = mean(
            &mut stats,
            self.q.max_age(),
        );
        self.tx.send(mean);
    }
    pub fn transfer_progress(&mut self) -> TransferProgressMonitor {
        TransferProgressMonitor::new(self.tx.subscribe())
    }
}

impl <W: Write> Write for InstantaneousProgressWriter<W> {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        let bytes_transferred = self.inner.write(buf)?;
        let slice = &buf[..bytes_transferred];
        self.update(slice);
        Ok(bytes_transferred)
    }
    fn flush(&mut self) -> Result<()> {
        self.inner.flush()
    }
}

fn count_bytes(buf: &[u8]) -> usize {
    buf.len()
}

fn count_lines(buf: &[u8]) -> usize {
    buf.iter()
        .filter(|b| 0xAu8 == **b)
        .count()
}

fn count_nulls(buf: &[u8]) -> usize {
    buf.iter()
        .filter(|b| 0x0u8 == **b)
        .count()
}
