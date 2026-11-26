//! The token of a session: the secret that a holder gives to the processes
//! of its session and to the daemon.

use std::fmt;

/// The token of one start of a session. A holder makes it from random bytes
/// at its start and gives it to the command of the session in the
/// `NOMUX_TOKEN` variable. A request to the daemon that has the token comes
/// from a process of that session.
///
/// The token is a secret. Its `Debug` output does not have its text, and no
/// code writes it to a log, to the journal or to the session metadata.
#[derive(Clone, Copy)]
pub struct SessionToken([u8; Self::LENGTH]);

impl SessionToken {
    /// The number of characters of a token.
    pub const LENGTH: usize = 32;
    /// The number of random bytes of a token: 128 bits.
    pub const RANDOM_LENGTH: usize = Self::LENGTH / 2;

    /// The token for `random` bytes: their lower-case hexadecimal digits.
    #[must_use]
    pub fn from_random(random: [u8; Self::RANDOM_LENGTH]) -> Self {
        const DIGITS: &[u8; 16] = b"0123456789abcdef";
        let mut text = [0; Self::LENGTH];
        for (pair, byte) in text.as_chunks_mut::<2>().0.iter_mut().zip(random) {
            pair[0] = DIGITS[usize::from(byte >> 4)];
            pair[1] = DIGITS[usize::from(byte & 0x0f)];
        }
        Self(text)
    }

    /// The text of the token, for the environment of the command of the
    /// session.
    ///
    /// # Panics
    ///
    /// Never: a token has only hexadecimal digits.
    #[must_use]
    pub fn as_str(&self) -> &str {
        str::from_utf8(&self.0).expect("a token has only hexadecimal digits")
    }

    /// `true` if `candidate` is this token. The time of the comparison does
    /// not depend on the position of the first byte that is different.
    #[must_use]
    pub fn matches(&self, candidate: &str) -> bool {
        let candidate = candidate.as_bytes();
        if candidate.len() != Self::LENGTH {
            return false;
        }
        let difference = self
            .0
            .iter()
            .zip(candidate)
            .fold(0, |difference, (own, other)| difference | (own ^ other));
        difference == 0
    }

    pub(super) fn encode(&self) -> &[u8; Self::LENGTH] {
        &self.0
    }

    /// Returns `None` if `bytes` are not the characters of a token.
    pub(super) fn decode(bytes: &[u8]) -> Option<Self> {
        let text: [u8; Self::LENGTH] = bytes.try_into().ok()?;
        let is_digit = |byte: &u8| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte);
        text.iter().all(is_digit).then_some(Self(text))
    }
}

impl PartialEq for SessionToken {
    fn eq(&self, other: &Self) -> bool {
        self.matches(other.as_str())
    }
}

impl Eq for SessionToken {}

impl fmt::Debug for SessionToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SessionToken(<redacted>)")
    }
}
