use std::num::NonZeroU32;

use super::config::Config;

#[derive(Debug)]
pub enum Event {
    Tick,
    Key,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Progress {
    pub bytes_transferred: usize,
    pub records_transferred: usize,
}

/// Something that has a configuration.
pub trait Configured {
    /// Get a copy of the configuration.
    fn config(&self) -> Config;
}

/// Something that can be reconfigured as-is.
pub trait Configurable {
    /// Replace the configuration with config.
    fn configure(&mut self, config: Config);
}

#[derive(Debug)]
pub enum PipeValve {
    /// A Pipe Valve that hasn't started yet.
    New(PipeValveNew),
    /// A Pipe Valve that has started transferring and will move data fast as it
    /// can until it finishes copying, is paused, cancelled, or fails.
    Running(PipeValveRunning),
    /// A Pipe Valve that is intentionally paused by the user and can be
    /// resumed.
    Paused(PipeValvePaused),
    /// A Pipe Valve that has been stopped due to an error or cancellation.
    Aborted(PipeValveAborted),
    /// A Pipe Valve that has completed its transfer successfully.
    Done,
}

impl Default for PipeValve {
    fn default() -> Self {
        Self::New(Default::default())
    }
}

impl PipeValve {
    pub fn active(&self) -> bool {
        match *self {
            Self::Running(_)
                | Self::Paused(_) => true,
            _ => false,
        }
    }
    pub fn start(self) -> Result<Self, ()> {
        match self {
            Self::New(new) => Ok(Self::Running(new.begin())),
            Self::Paused(paused) => Ok(Self::Running(paused.resume())),
            _ => Err(()),
        }
    }
    pub fn process(self, event: Event) -> Result<Self, ()> {
        dbg!(event);
        Ok(self)
    }
}

#[derive(Debug,Default)]
pub struct PipeValveNew {
    config: Config,
}
#[derive(Debug)]
pub struct PipeValveRunning {
    config: Config,
    progress: Progress,
}
#[derive(Debug)]
pub struct PipeValvePaused {
    config: Config,
    progress: Progress,
}
#[derive(Debug, Default)]
pub struct PipeValveAborted {
    progress: Option<Progress>,
}

impl PipeValveNew {
    pub fn begin(self) -> PipeValveRunning {
        PipeValveRunning {
            config: self.config,
            progress: Default::default(),
        }
    }
    pub fn set_limit(self, limit: Option<NonZeroU32>) -> PipeValveNew {
        Self {
            config: Config {
                limit,
                ..self.config
            },
        }
    }
    pub fn abort(self) -> PipeValveAborted {
        Default::default()
    }
}

impl PipeValveRunning {
    pub fn pause(self) -> PipeValvePaused {
        PipeValvePaused {
            config: self.config,
            progress: self.progress,
        }
    }
    pub fn abort(self) -> PipeValveAborted {
        PipeValveAborted {
            progress: Some(self.progress),
        }
    }
}

impl PipeValvePaused {
    pub fn resume(self) -> PipeValveRunning {
        PipeValveRunning {
            config: self.config,
            progress: Default::default(),
        }
    }
    fn abort(self) -> PipeValveAborted {
        PipeValveAborted {
            progress: Some(self.progress),
        }
    }
}

impl Configured for PipeValveNew {
    fn config(&self) -> Config {
        self.config
    }
}

impl Configured for PipeValvePaused {
    fn config(&self) -> Config {
        self.config
    }
}

impl Configured for PipeValveRunning {
    fn config(&self) -> Config {
        self.config
    }
}

#[test]
fn test() {
    let pv = PipeValveNew::default();
    dbg!(&pv);
    let pv = pv.begin();
    dbg!(&pv);
    let pv = pv.pause();
    dbg!(&pv);
    let pv = pv.resume();
    dbg!(&pv);
    let pv = pv.abort();
    dbg!(&pv);
}
