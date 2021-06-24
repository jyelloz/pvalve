use watch::WatchReceiver;

#[derive(Debug, Default, Clone, Copy, PartialEq, PartialOrd)]
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

impl TransferProgressMonitor {
    pub fn new(rx: WatchReceiver<TransferProgress>) -> Self {
        Self(rx)
    }
    pub fn get(&mut self) -> TransferProgress {
        self.0.get()
    }
}
