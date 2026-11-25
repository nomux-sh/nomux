//! The requests: the messages from the daemon to the holder.
//!
//! ```text
//! kind    request       payload
//! 0x0101  AcquireLease  empty
//! 0x0102  Input         the bytes for the terminal
//! 0x0103  Resize        rows (2), columns (2)
//! 0x0104  Signal        signal (1): 1 hangup, 2 interrupt, 3 terminate, 4 kill
//! 0x0105  Status        empty
//! 0x0106  WaitOutput    write position that the daemon already has (8)
//! ```

use super::frame::Frame;
use super::payload::{DecodeError, PayloadReader};
use super::window_size::WindowSize;

const ACQUIRE_LEASE: u16 = 0x0101;
const INPUT: u16 = 0x0102;
const RESIZE: u16 = 0x0103;
const SIGNAL: u16 = 0x0104;
const STATUS: u16 = 0x0105;
const WAIT_OUTPUT: u16 = 0x0106;

/// A signal that the daemon can send to the child command.
///
/// The values are protocol values, not signal numbers of the operating
/// system.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChildSignal {
    /// `SIGHUP`.
    Hangup,
    /// `SIGINT`.
    Interrupt,
    /// `SIGTERM`.
    Terminate,
    /// `SIGKILL`.
    Kill,
}

impl ChildSignal {
    fn encode(self) -> u8 {
        match self {
            Self::Hangup => 1,
            Self::Interrupt => 2,
            Self::Terminate => 3,
            Self::Kill => 4,
        }
    }

    fn decode(value: u8) -> Result<Self, DecodeError> {
        match value {
            1 => Ok(Self::Hangup),
            2 => Ok(Self::Interrupt),
            3 => Ok(Self::Terminate),
            4 => Ok(Self::Kill),
            _ => Err(DecodeError::InvalidPayload),
        }
    }
}

/// A message from the daemon to the holder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request<'payload> {
    /// Take the single-writer lease for this connection. The connection that
    /// had the lease loses it.
    AcquireLease,
    /// Write bytes to the terminal. Needs the lease.
    Input(&'payload [u8]),
    /// Change the size of the terminal window. Needs the lease.
    Resize(WindowSize),
    /// Send a signal to the child command. Needs the lease.
    Signal(ChildSignal),
    /// Ask if the child command runs or how it ended.
    Status,
    /// Answer when the write position of the ring buffer is larger than
    /// `after`, or when the child command has ended.
    WaitOutput {
        /// The write position that the daemon already has.
        after: u64,
    },
}

impl<'payload> Request<'payload> {
    /// Appends the encoded request frame to `output`.
    ///
    /// # Panics
    ///
    /// Panics if the bytes of an [`Request::Input`] are longer than
    /// [`MAX_PAYLOAD_LENGTH`](super::MAX_PAYLOAD_LENGTH).
    pub fn encode(&self, request_id: u32, output: &mut Vec<u8>) {
        let mut fields = [0u8; 8];
        let (kind, payload): (u16, &[u8]) = match *self {
            Self::AcquireLease => (ACQUIRE_LEASE, &[]),
            Self::Input(bytes) => (INPUT, bytes),
            Self::Resize(size) => {
                fields[..WindowSize::ENCODED_LENGTH].copy_from_slice(&size.encode());
                (RESIZE, &fields[..WindowSize::ENCODED_LENGTH])
            }
            Self::Signal(signal) => {
                fields[0] = signal.encode();
                (SIGNAL, &fields[..1])
            }
            Self::Status => (STATUS, &[]),
            Self::WaitOutput { after } => {
                fields = after.to_le_bytes();
                (WAIT_OUTPUT, &fields)
            }
        };
        let frame = Frame {
            kind,
            request_id,
            payload,
        };
        frame.encode(output);
    }

    /// Decodes a request frame.
    ///
    /// # Errors
    ///
    /// Returns an error if this version does not know the kind, or if the
    /// payload is not valid for the kind.
    pub fn decode(frame: Frame<'payload>) -> Result<Self, DecodeError> {
        let mut reader = PayloadReader::new(frame.payload);
        match frame.kind {
            ACQUIRE_LEASE => Ok(Self::AcquireLease),
            INPUT => Ok(Self::Input(frame.payload)),
            RESIZE => WindowSize::decode(&mut reader).map(Self::Resize),
            SIGNAL => ChildSignal::decode(reader.u8()?).map(Self::Signal),
            STATUS => Ok(Self::Status),
            WAIT_OUTPUT => Ok(Self::WaitOutput {
                after: reader.u64()?,
            }),
            kind => Err(DecodeError::UnknownKind(kind)),
        }
    }
}
