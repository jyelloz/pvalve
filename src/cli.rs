use std::num::NonZeroU32;

use gumdrop::Options;

use super::config::Unit;

#[derive(Debug, Clone, Copy, PartialOrd, Ord, PartialEq, Eq)]
pub struct Speed(pub NonZeroU32);

impl std::str::FromStr for Speed {
    type Err = std::num::ParseIntError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        NonZeroU32::from_str(s).map(|i| Self(i))
    }
}

impl Into<NonZeroU32> for &Speed {
    fn into(self) -> NonZeroU32 {
        self.0
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Invocation {
    pub speed: Option<Speed>,
    pub unit: Unit,
}

#[derive(Debug, Default, Clone, Copy, Options)]
pub struct Opts {
    #[options(
        short = "L",
        help = "the maximum number of items to allow through per second",
    )]
    speed_limit: Option<Speed>,
    #[options(
        short = "l",
        help = "enable line-oriented mode where measurements apply to lines, not bytes",
    )]
    line_mode: bool,
    #[options(
        short = "0",
        long = "null",
        help = "similar to line-oriented mode, except the lines are NUL-separated"
    )]
    null_mode: bool,
    help: bool,
}

impl Opts {
    pub fn parse_process_args() -> Invocation {
        let opts = Self::parse_args_default_or_exit();
        opts.into()
    }
}

impl From<&Opts> for Unit {
    fn from(opts: &Opts) -> Self {
        if opts.null_mode {
            Self::Null
        } else if opts.line_mode {
            Self::Line
        } else {
            Self::Byte
        }
    }
}

impl From<Opts> for Invocation {
    fn from(opts: Opts) -> Self {
        let unit = Unit::from(&opts);
        let speed = opts.speed_limit;
        Self { unit, speed }
    }
}

#[cfg(test)]
#[allow(non_snake_case)]
mod tests {
    use super::*;
    type Result = anyhow::Result<()>;
    #[test]
    fn when__unit_not_selected__then__bytes_is_used() -> Result {
        let opts = Opts::parse_args_default::<&str>(&[])?;
        let unit = Invocation::from(opts).unit;
        assert_eq!(unit, Unit::Byte);
        Ok(())
    }

    #[test]
    fn when__line_unit_selected__then__line_is_used() -> Result {
        let opts = Opts::parse_args_default(&["-l"])?;
        let unit = Invocation::from(opts).unit;
        assert_eq!(unit, Unit::Line);
        Ok(())
    }

    #[test]
    fn when__null_unit_selected__then__null_is_used() -> Result {
        let opts = Opts::parse_args_default(&["-0"])?;
        let unit = Invocation::from(opts).unit;
        assert_eq!(unit, Unit::Null);
        Ok(())
    }

    #[test]
    fn when__line_and_null_units_selected__then__null_is_used() -> Result {
        let opts = Opts::parse_args_default(&["-l", "-0"])?;
        let unit = Invocation::from(opts).unit;
        assert_eq!(unit, Unit::Null);
        Ok(())
    }

    #[test]
    fn when__null_and_line_units_selected__then__null_is_used() -> Result {
        let opts = Opts::parse_args_default(&["-0", "-l"])?;
        let unit = Invocation::from(opts).unit;
        assert_eq!(unit, Unit::Null);
        Ok(())
    }
}
