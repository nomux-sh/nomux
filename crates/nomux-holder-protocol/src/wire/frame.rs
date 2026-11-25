//! The frame: the unit of the control socket.
//!
//! ```text
//! offset  size  field
//!      0     4  length: the number of bytes after this field (8 + payload)
//!      4     2  kind
//!      6     2  flags: the sender writes 0, the receiver ignores the value
//!      8     4  request identifier
//!     12     n  payload
//! ```
//!
//! All integers are little-endian.

use std::error::Error;
use std::fmt;

/// The largest number of payload bytes in one frame.
pub const MAX_PAYLOAD_LENGTH: usize = 64 * 1024;

const LENGTH_FIELD: usize = 4;
/// The bytes that the length field counts before the payload.
const FIXED_FIELDS: usize = 8;
const MAX_FRAME_LENGTH: usize = LENGTH_FIELD + FIXED_FIELDS + MAX_PAYLOAD_LENGTH;

/// One message on the control socket, before its payload is decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Frame<'payload> {
    /// The message kind.
    pub kind: u16,
    /// The identifier that connects a reply to its request.
    pub request_id: u32,
    /// The bytes of the message.
    pub payload: &'payload [u8],
}

impl Frame<'_> {
    /// Appends the encoded frame to `output`.
    ///
    /// # Panics
    ///
    /// Panics if the payload is longer than [`MAX_PAYLOAD_LENGTH`].
    pub fn encode(&self, output: &mut Vec<u8>) {
        assert!(
            self.payload.len() <= MAX_PAYLOAD_LENGTH,
            "a frame payload has {MAX_PAYLOAD_LENGTH} bytes at most"
        );
        let length = u32::try_from(FIXED_FIELDS + self.payload.len())
            .expect("the maximum frame length fits in 32 bits");
        output.extend_from_slice(&length.to_le_bytes());
        output.extend_from_slice(&self.kind.to_le_bytes());
        output.extend_from_slice(&0u16.to_le_bytes());
        output.extend_from_slice(&self.request_id.to_le_bytes());
        output.extend_from_slice(self.payload);
    }
}

/// The length field of a frame is not possible. The byte stream cannot be
/// read further, thus the receiver must close the connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameError {
    /// The value of the length field.
    pub length: u32,
}

impl fmt::Display for FrameError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "frame length {} is not permitted", self.length)
    }
}

impl Error for FrameError {}

/// Collects the bytes of a stream socket and gives back complete frames.
///
/// Use it in this sequence: read from the socket into [`unfilled`], call
/// [`advance`] with the number of bytes read, then call [`next_frame`] until
/// it returns `None`.
///
/// [`unfilled`]: FrameBuffer::unfilled
/// [`advance`]: FrameBuffer::advance
/// [`next_frame`]: FrameBuffer::next_frame
#[derive(Debug)]
pub struct FrameBuffer {
    bytes: Box<[u8]>,
    start: usize,
    end: usize,
}

impl FrameBuffer {
    /// Makes an empty buffer that can hold one frame of the maximum length.
    #[must_use]
    pub fn new() -> Self {
        Self {
            bytes: vec![0; MAX_FRAME_LENGTH].into_boxed_slice(),
            start: 0,
            end: 0,
        }
    }

    /// The free part of the buffer. It is not empty when [`next_frame`]
    /// returned `None`.
    ///
    /// [`next_frame`]: FrameBuffer::next_frame
    pub fn unfilled(&mut self) -> &mut [u8] {
        if self.start > 0 {
            self.bytes.copy_within(self.start..self.end, 0);
            self.end -= self.start;
            self.start = 0;
        }
        &mut self.bytes[self.end..]
    }

    /// Records that `count` bytes were written to the start of [`unfilled`].
    ///
    /// # Panics
    ///
    /// Panics if `count` is larger than the free part of the buffer.
    ///
    /// [`unfilled`]: FrameBuffer::unfilled
    pub fn advance(&mut self, count: usize) {
        assert!(count <= self.bytes.len() - self.end);
        self.end += count;
    }

    /// Takes the next complete frame out of the buffer.
    ///
    /// # Errors
    ///
    /// Returns an error if the length field is smaller than the fixed fields
    /// or larger than the maximum frame.
    pub fn next_frame(&mut self) -> Result<Option<Frame<'_>>, FrameError> {
        let available = &self.bytes[self.start..self.end];
        let Some((length_field, rest)) = available.split_first_chunk::<LENGTH_FIELD>() else {
            return Ok(None);
        };
        let length = u32::from_le_bytes(*length_field);
        let body_length = usize::try_from(length).map_err(|_| FrameError { length })?;
        if !(FIXED_FIELDS..=FIXED_FIELDS + MAX_PAYLOAD_LENGTH).contains(&body_length) {
            return Err(FrameError { length });
        }
        if rest.len() < body_length {
            return Ok(None);
        }
        let frame_start = self.start;
        self.start += LENGTH_FIELD + body_length;
        let body = &self.bytes[frame_start + LENGTH_FIELD..self.start];
        Ok(Some(Frame {
            kind: u16::from_le_bytes([body[0], body[1]]),
            request_id: u32::from_le_bytes([body[4], body[5], body[6], body[7]]),
            payload: &body[FIXED_FIELDS..],
        }))
    }
}

impl Default for FrameBuffer {
    fn default() -> Self {
        Self::new()
    }
}
