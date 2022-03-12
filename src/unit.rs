#[derive(Debug, Clone, Copy, PartialOrd, Ord, PartialEq, Eq)]
pub enum Unit {
    Byte,
    Line,
    Null,
}

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
