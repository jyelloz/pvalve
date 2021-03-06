use std::num::NonZeroU32;

#[derive(Debug, Clone)]
pub enum Message {
    UpdateRate(NonZeroU32),
    Interrupted,
}

#[derive(Debug, Clone)]
pub enum ProgressMessage {
    Initial,
    Interrupted,
}
