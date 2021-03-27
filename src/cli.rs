use gumdrop::Options;

use std::{
    num::{NonZeroU32, ParseIntError},
    str::FromStr,
};

#[derive(Debug, PartialEq, Eq)]
pub struct Speed(pub NonZeroU32);

impl FromStr for Speed {
    type Err = ParseIntError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        NonZeroU32::from_str(s).map(|i| Self(i))
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Mode {
    Bytes,
    Lines,
    Nulls,
}

impl Default for Mode {
    fn default() -> Self {
        Self::Bytes
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Invocation {
    pub speed: Option<Speed>,
    pub mode: Mode,
}

#[derive(Debug, Options)]
pub struct Opts {
    #[options(short = "L")]
    speed_limit: Option<Speed>,
    #[options(
        help = "enable line-oriented mode where measurements apply to lines, not bytes"
    )]
    line_mode: bool,
    #[options(
        short = "0",
        long = "null",
        help = "same as line-oriented mode, except the lines are NUL-separated"
    )]
    null_mode: bool,
    help: bool,
}

impl Opts {
    pub fn parse_process_args() -> Invocation {
        let opts = Self::parse_args_default_or_exit();
        Invocation::from(opts)
    }
}

impl From<&Opts> for Mode {
    fn from(opts: &Opts) -> Self {
        if opts.null_mode {
            Self::Nulls
        } else if opts.line_mode {
            Self::Lines
        } else {
            Self::Bytes
        }
    }
}

impl From<Opts> for Invocation {
    fn from(opts: Opts) -> Self {
        let mode = Mode::from(&opts);
        let speed = opts.speed_limit;
        Invocation { mode, speed }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn when__mode_not_selected__then__bytes_is_used() -> anyhow::Result<()> {
        let opts = Opts::parse_args_default::<&str>(&[])?;
        let mode = Invocation::from(opts).mode;
        assert_eq!(mode, Mode::Bytes,);
        Ok(())
    }

    #[test]
    fn when__line_mode_selected__then__lines_is_used() -> anyhow::Result<()> {
        let opts = Opts::parse_args_default(&["-l"])?;
        let mode = Invocation::from(opts).mode;
        assert_eq!(mode, Mode::Lines,);
        Ok(())
    }

    #[test]
    fn when__null_mode_selected__then__nulls_is_used() -> anyhow::Result<()> {
        let opts = Opts::parse_args_default(&["-0"])?;
        let mode = Invocation::from(opts).mode;
        assert_eq!(mode, Mode::Nulls,);
        Ok(())
    }
}
