use std::num::NonZeroU32;

use clap::{AppSettings, Parser};

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

/// Pipe Valve - Monitor and control pipe throughput.
#[derive(Debug, Default, Clone, Copy, Parser)]
#[clap(version)]
#[clap(global_setting(AppSettings::DeriveDisplayOrder))]
pub struct Opts {
    #[clap(
        short = 'L',
        help = "Limit the throughput of the transfer.",
    )]
    speed_limit: Option<Speed>,
    #[clap(
        short = 'l',
        help = "Measurements apply to line-separated records.",
    )]
    line_mode: bool,
    #[clap(
        short = '0',
        long = "null",
        help = "Measurements apply to null-separated records.",
    )]
    null_mode: bool,
}

impl Opts {
    pub fn parse_process_args() -> Invocation {
        let opts = Self::parse();
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

    fn parse(args: &[&str]) -> anyhow::Result<Invocation> {
        let args = [&["pvalve"][..], args].concat();
        let invo = Opts::try_parse_from(args)?;
        Ok(invo.into())
    }

    #[test]
    fn when__unit_not_selected__then__bytes_is_used() -> Result {
        let Invocation { unit, .. } = parse(&[])?;
        assert_eq!(unit, Unit::Byte);
        Ok(())
    }

    #[test]
    fn when__line_unit_selected__then__line_is_used() -> Result {
        let Invocation { unit, .. } = parse(&["-l"])?;
        assert_eq!(unit, Unit::Line);
        Ok(())
    }

    #[test]
    fn when__null_unit_selected__then__null_is_used() -> Result {
        let Invocation { unit, .. } = parse(&["-0"])?;
        assert_eq!(unit, Unit::Null);
        Ok(())
    }

    #[test]
    fn when__line_and_null_units_selected__then__null_is_used() -> Result {
        let Invocation { unit, .. } = parse(&["-l", "-0"])?;
        assert_eq!(unit, Unit::Null);
        Ok(())
    }

    #[test]
    fn when__null_and_line_units_selected__then__null_is_used() -> Result {
        let Invocation { unit, .. } = parse(&["-0", "-l"])?;
        assert_eq!(unit, Unit::Null);
        Ok(())
    }
}
