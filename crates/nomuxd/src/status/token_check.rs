//! The check of the token of a session: a request proves with it that it
//! comes from a process of the session.

use std::error::Error;
use std::fmt;

/// Why the daemon refused a request of a program in a session: the request
/// did not prove that it comes from the session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TokenRefused {
    /// The token of the request is not the token of the session.
    WrongToken,
    /// The holder of the session is too old to make a token.
    HolderWithoutToken,
    /// The session has no process, thus no process can send a request for
    /// it.
    NoProcess,
}

impl fmt::Display for TokenRefused {
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

impl Error for TokenRefused {}
