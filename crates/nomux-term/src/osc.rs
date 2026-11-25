//! An OSC sequence in the output of a program.
//!
//! The VT library has no hook for the OSC codes that it does not know. The
//! [`SequenceScanner`](crate::sequence_scanner::SequenceScanner) finds the
//! OSC sequences.

/// An operating system command (OSC) from the output of a program:
/// `ESC ] code ; payload`, with BEL or `ESC \` at the end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Osc<'a> {
    /// The number before the first `;`, for example 133 or 7501.
    pub code: u32,
    /// The bytes after the first `;`. Empty if the sequence has no `;`.
    pub payload: &'a [u8],
}

impl<'a> Osc<'a> {
    /// Returns `None` if the sequence does not start with a decimal number.
    pub(crate) fn parse(sequence: &'a [u8]) -> Option<Self> {
        let separator = sequence.iter().position(|byte| *byte == b';');
        let (digits, payload) = match separator {
            Some(index) => (&sequence[..index], &sequence[index + 1..]),
            None => (sequence, &[][..]),
        };
        let code = std::str::from_utf8(digits).ok()?.parse().ok()?;
        Some(Self { code, payload })
    }
}
