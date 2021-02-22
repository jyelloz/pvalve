use std::{num::NonZeroU32, time::Duration};

#[derive(Debug, Clone)]
pub enum Message {
    UpdateRate(NonZeroU32),
    Interrupted,
}

#[derive(Debug, Clone)]
pub enum ProgressMessage {
    Initial,
    Transfer(usize, Duration),
    Interrupted,
}
