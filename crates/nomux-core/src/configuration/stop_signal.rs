//! A signal of the sequence that ends the command of an agent session
//! before a hibernation.

use std::fmt;
use std::str::FromStr;

/// A signal that the daemon can send to the command of a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopSignal {
    /// `SIGHUP`: the signal that the end of a terminal gives.
    Hangup,
    /// `SIGINT`: the signal of Ctrl-C.
    Interrupt,
    /// `SIGTERM`: the request to end.
    Terminate,
    /// `SIGKILL`: the end that a program cannot refuse.
    Kill,
}

impl StopSignal {
    /// The name of the signal in the configuration file.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Hangup => "HUP",
            Self::Interrupt => "INT",
            Self::Terminate => "TERM",
            Self::Kill => "KILL",
        }
    }
}

impl fmt::Display for StopSignal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for StopSignal {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        [Self::Hangup, Self::Interrupt, Self::Terminate, Self::Kill]
            .into_iter()
            .find(|signal| signal.as_str() == text)
            .ok_or_else(|| format!("{text:?} is not HUP, INT, TERM or KILL"))
    }
}
