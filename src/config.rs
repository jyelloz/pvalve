use std::{
    num::NonZeroU32,
    sync::{Arc, RwLock},
};

use nonzero_ext::nonzero;

use lazy_static::lazy_static;

lazy_static! {
    static ref CURRENT_CONFIG: Arc<RwLock<Config>> = Arc::new(RwLock::new(
        Default::default()
    ));
}

#[derive(Debug, Default, Clone)]
pub struct Config {
    pub paused: bool,
    pub limit: Option<NonZeroU32>,
}

impl Config {
    pub fn current() -> Config {
        CURRENT_CONFIG.read().unwrap().clone()
    }
    pub fn make_current(&self) {
        let mut write = CURRENT_CONFIG.write().unwrap();
        *write = self.clone();
    }
    pub fn limit(&self) -> NonZeroU32 {
        self.limit.unwrap_or(nonzero!(1u32))
    }
}
