//! Reads and writes the fields of a payload, and of the program reports in
//! shared memory.

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
pub(crate) struct PayloadReader<'payload> {
    remaining: &'payload [u8],
}

impl<'payload> PayloadReader<'payload> {
    pub(crate) fn new(payload: &'payload [u8]) -> Self {
        Self { remaining: payload }
    }

    pub(crate) fn bytes(&mut self, count: usize) -> Result<&'payload [u8], DecodeError> {
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

    pub(crate) fn u8(&mut self) -> Result<u8, DecodeError> {
        self.array().map(u8::from_le_bytes)
    }

    pub(crate) fn u16(&mut self) -> Result<u16, DecodeError> {
        self.array().map(u16::from_le_bytes)
    }

    pub(crate) fn u32(&mut self) -> Result<u32, DecodeError> {
        self.array().map(u32::from_le_bytes)
    }

    pub(crate) fn i32(&mut self) -> Result<i32, DecodeError> {
        self.array().map(i32::from_le_bytes)
    }

    pub(crate) fn u64(&mut self) -> Result<u64, DecodeError> {
        self.array().map(u64::from_le_bytes)
    }

    /// A block: a length (2) and that number of bytes. The reader of the
    /// block ignores the bytes that a later version adds to its end.
    pub(crate) fn block(&mut self) -> Result<Self, DecodeError> {
        let length = usize::from(self.u16()?);
        self.bytes(length).map(Self::new)
    }

    /// A text: a block of UTF-8.
    pub(crate) fn text(&mut self) -> Result<String, DecodeError> {
        String::from_utf8(self.block()?.remaining.to_vec()).map_err(|_| DecodeError::InvalidPayload)
    }
}

/// Appends a block to `output`: the length (2) of the bytes that `write`
/// appends, and those bytes.
///
/// # Panics
///
/// Panics if `write` appends more than 65535 bytes.
pub(crate) fn write_block(output: &mut Vec<u8>, write: impl FnOnce(&mut Vec<u8>)) {
    let length_field = output.len();
    output.extend_from_slice(&[0; 2]);
    write(output);
    let length =
        u16::try_from(output.len() - length_field - 2).expect("a block has 65535 bytes at most");
    output[length_field..length_field + 2].copy_from_slice(&length.to_le_bytes());
}

/// Appends a text to `output` as a block.
///
/// # Panics
///
/// Panics if `text` is longer than 65535 bytes.
pub(crate) fn write_text(output: &mut Vec<u8>, text: &str) {
    write_block(output, |block| block.extend_from_slice(text.as_bytes()));
}
