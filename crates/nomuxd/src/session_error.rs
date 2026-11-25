//! Why an operation on a session did not succeed.

use std::error::Error;
use std::fmt;

#[derive(Debug)]
pub(crate) enum SessionError {
    NotFound(String),
    Timeout(String),
    Failed(String),
}

impl fmt::Display for SessionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (Self::NotFound(message) | Self::Timeout(message) | Self::Failed(message)) = self;
        formatter.write_str(message)
    }
}

impl Error for SessionError {}
