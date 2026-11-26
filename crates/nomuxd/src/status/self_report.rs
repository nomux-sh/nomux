//! The self-report: what a program in a session reports about its session
//! with a request to the daemon.

use std::error::Error;
use std::fmt;

use nomux_status::{Observation, State};

use super::holder_sources::MAX_MESSAGE_LENGTH;

/// A self-report of a request, after its checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SelfReport {
    /// The session has this status.
    Status(Observation),
    /// The session has no self-report any more.
    Withdrawn,
}

impl SelfReport {
    /// The self-report for `state` and `message`. [`State::Idle`] takes the
    /// self-report back: an idle session is a session that nothing reports.
    ///
    /// # Errors
    ///
    /// Returns the reason if `message` is not one short line of text.
    pub(crate) fn new(state: State, message: &str) -> Result<Self, String> {
        if message.len() > MAX_MESSAGE_LENGTH {
            return Err(format!(
                "the message has {} bytes: {MAX_MESSAGE_LENGTH} at most",
                message.len()
            ));
        }
        if message.chars().any(char::is_control) {
            return Err(
                "the message must be one line of text without a control character".to_owned(),
            );
        }
        if state == State::Idle {
            return Ok(Self::Withdrawn);
        }
        Ok(Self::Status(Observation {
            message: (!message.is_empty()).then(|| message.to_owned()),
            ..Observation::of(state)
        }))
    }
}

/// Why the daemon did not take a self-report: the request did not prove
/// that it comes from the session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SelfReportRefused {
    /// The token of the request is not the token of the session.
    WrongToken,
    /// The holder of the session is too old to make a token.
    HolderWithoutToken,
    /// The session has no process, thus no process can report for it.
    NoProcess,
}

impl fmt::Display for SelfReportRefused {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::WrongToken => "the token of the request is not the token of the session",
            Self::HolderWithoutToken => {
                "the holder of the session is from an earlier nomux version and has no token: \
                 the session cannot report its status until it starts again"
            }
            Self::NoProcess => "the session has no process that the daemon knows",
        })
    }
}

impl Error for SelfReportRefused {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_is_one_short_line_of_text() {
        let working = |message: &str| SelfReport::new(State::Working, message);

        assert!(working("tests run").is_ok());
        assert!(working(&"a".repeat(MAX_MESSAGE_LENGTH)).is_ok());
        assert!(working(&"a".repeat(MAX_MESSAGE_LENGTH + 1)).is_err());
        assert!(working("two\nlines").is_err());
        assert!(working("escape \x1b[31m").is_err());
    }

    #[test]
    fn idle_takes_the_self_report_back() {
        assert_eq!(SelfReport::new(State::Idle, ""), Ok(SelfReport::Withdrawn));
        let Ok(SelfReport::Status(done)) = SelfReport::new(State::Done, "") else {
            panic!("done is a status");
        };
        assert_eq!((done.state, done.message), (State::Done, None));
    }
}
