//! Reads the fixed fields of a payload.

use std::error::Error;
use std::fmt;

/// A frame is complete, but it is not a message that this version can use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeError {
    /// This version does not know the message kind.
    UnknownKind(u16),
    /// The payload is too short, or a field has a value that is not permitted.
    InvalidPayload,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownKind(kind) => write!(formatter, "unknown message kind {kind:#06x}"),
            Self::InvalidPayload => formatter.write_str("invalid message payload"),
        }
    }
}

impl Error for DecodeError {}

/// Takes fields from the start of a payload. The bytes that no field takes
/// are ignored: a later protocol version can add fields there.
pub(super) struct PayloadReader<'payload> {
    remaining: &'payload [u8],
}

impl<'payload> PayloadReader<'payload> {
    pub(super) fn new(payload: &'payload [u8]) -> Self {
        Self { remaining: payload }
    }

    pub(super) fn bytes(&mut self, count: usize) -> Result<&'payload [u8], DecodeError> {
        let (taken, remaining) = self
            .remaining
            .split_at_checked(count)
            .ok_or(DecodeError::InvalidPayload)?;
        self.remaining = remaining;
        Ok(taken)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], DecodeError> {
        let (taken, remaining) = self
            .remaining
            .split_first_chunk::<N>()
            .ok_or(DecodeError::InvalidPayload)?;
        self.remaining = remaining;
        Ok(*taken)
    }

    pub(super) fn u8(&mut self) -> Result<u8, DecodeError> {
        self.array().map(u8::from_le_bytes)
    }

    pub(super) fn u16(&mut self) -> Result<u16, DecodeError> {
        self.array().map(u16::from_le_bytes)
    }

    pub(super) fn u32(&mut self) -> Result<u32, DecodeError> {
        self.array().map(u32::from_le_bytes)
    }

    pub(super) fn i32(&mut self) -> Result<i32, DecodeError> {
        self.array().map(i32::from_le_bytes)
    }

    pub(super) fn u64(&mut self) -> Result<u64, DecodeError> {
        self.array().map(u64::from_le_bytes)
    }
}
