//! The self-report: what a program in a session reports about its session
//! with a request to the daemon.

use nomux_status::{Observation, State};

use super::report_text::one_line;

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
        let message = one_line("message", message)?;
        if state == State::Idle {
            return Ok(Self::Withdrawn);
        }
        Ok(Self::Status(Observation {
            message,
            ..Observation::of(state)
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::super::holder_sources::MAX_MESSAGE_LENGTH;
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
