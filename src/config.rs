use std::num::NonZeroU32;

use nonzero_ext::nonzero;

use watch::{WatchReceiver, WatchSender, channel};

#[derive(Debug, Default, Clone)]
pub struct Config {
    pub limit: Option<NonZeroU32>,
}

#[derive(Clone)]
pub struct ConfigMonitor(WatchReceiver<Config>);

pub struct Latch {
    active: bool,
    tx: WatchSender<bool>,
}
pub struct LatchMonitor(WatchReceiver<bool>);

impl Config {
    pub fn limit(&self) -> NonZeroU32 {
        self.limit.unwrap_or(nonzero!(1u32))
    }
}

impl ConfigMonitor {
    pub fn new(config: Config) -> (WatchSender<Config>, Self) {
        let (tx, rx) = channel(config);
        (tx, Self(rx))
    }
    pub fn limit_if_new(&mut self) -> Option<NonZeroU32> {
        self.0
            .get_if_new()
            .map(|config| config.limit())
    }
    pub fn limit(&mut self) -> NonZeroU32 {
        self.0.get().limit()
    }
}

impl Latch {
    pub fn new() -> Self {
        let active = false;
        let (tx, _) = channel(active);
        Self {
            active,
            tx,
        }
    }
    pub fn active(&self) -> bool {
        self.active
    }
    pub fn toggle(&mut self) {
        self.active = !self.active;
        self.tx();
    }
    pub fn on(&mut self) {
        self.active = true;
        self.tx();
    }
    pub fn off(&mut self) {
        self.active = false;
        self.tx();
    }
    fn tx(&mut self) {
        self.tx.send(self.active);
    }
    pub fn watch(&mut self) -> LatchMonitor {
        LatchMonitor(self.tx.subscribe())
    }
}

impl LatchMonitor {
    pub fn active(&mut self) -> bool {
        self.0.get()
    }
}
