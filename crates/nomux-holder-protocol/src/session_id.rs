//! The identifier of a session.

use std::error::Error;
use std::fmt;
use std::str::FromStr;

/// The identifier of one session on one host.
///
/// The identifier is the name of the discovery directory of the holder, thus
/// it contains only ASCII letters, digits, `-` and `_`, and it has 1 to
/// [`SessionId::MAX_LENGTH`] bytes.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SessionId(String);

impl SessionId {
    /// The largest number of bytes in an identifier.
    pub const MAX_LENGTH: usize = 64;

    /// The identifier as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for SessionId {
    type Err = InvalidSessionId;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let permitted = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_';
        if text.is_empty() || text.len() > Self::MAX_LENGTH || !text.bytes().all(permitted) {
            return Err(InvalidSessionId);
        }
        Ok(Self(text.to_owned()))
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// The text is not a session identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidSessionId;

impl fmt::Display for InvalidSessionId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "a session identifier has 1 to {} characters from A-Z, a-z, 0-9, '-' and '_'",
            SessionId::MAX_LENGTH
        )
    }
}

impl Error for InvalidSessionId {}
