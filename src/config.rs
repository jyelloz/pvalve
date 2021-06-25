use std::num::NonZeroU32;

use nonzero_ext::nonzero;

use watch::{
    WatchReceiver,
    WatchSender,
    channel,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SpeedLimit {
    limit: NonZeroU32,
    enabled: bool,
}

#[derive(Default, Debug, Clone, Copy)]
pub struct Config {
    pub limit: SpeedLimit,
    pub unit: Unit,
}

#[derive(Debug, Clone, Copy, PartialOrd, Ord, PartialEq, Eq)]
pub enum Unit {
    Byte,
    Line,
    Null,
}

#[derive(Clone)]
pub struct ConfigMonitor(WatchReceiver<Config>);

#[derive(Clone)]
pub struct Latch {
    active: bool,
    tx: WatchSender<bool>,
}
#[derive(Clone)]
pub struct LatchMonitor(WatchReceiver<bool>);

impl Default for Unit {
    fn default() -> Self {
        Self::Byte
    }
}

impl Unit {
    pub fn cycle(&mut self) {
        *self = match self {
            Self::Byte => Self::Line,
            Self::Line => Self::Null,
            Self::Null => Self::Byte,
        }
    }
}

impl Default for SpeedLimit {
    fn default() -> Self {
        Self {
            limit: nonzero!(1u32),
            enabled: false,
        }
    }
}

impl SpeedLimit {
    fn limit(&self) -> Option<NonZeroU32> {
        if self.enabled {
            Some(self.limit)
        } else {
            None
        }
    }
    fn toggle(&mut self) -> bool {
        let enabled = self.enabled;
        self.enabled = !enabled;
        enabled
    }
}

impl From<Option<NonZeroU32>> for SpeedLimit {
    fn from(limit: Option<NonZeroU32>) -> Self {
        if let Some(limit) = limit {
            Self {
                limit,
                enabled: true,
            }
        } else {
            Self::default()
        }
    }
}

impl Config {
    pub fn limit(&self) -> Option<NonZeroU32> {
        self.limit.limit()
    }
    pub fn toggle_limit(&mut self) -> bool {
        self.limit.toggle()
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
            .and_then(|config| config.limit())
    }
    pub fn limit(&mut self) -> Option<NonZeroU32> {
        self.0
            .get()
            .limit()
    }
    pub fn unit(&mut self) -> Unit {
        self.0.get().unit
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
