//! The requests: the messages from the daemon to the holder.
//!
//! ```text
//! kind    request             payload
//! 0x0101  AcquireLease        empty
//! 0x0102  Input               the bytes for the terminal
//! 0x0103  Resize              rows (2), columns (2)
//! 0x0104  Signal              signal (1): 1 hangup, 2 interrupt, 3 terminate,
//!                             4 kill
//! 0x0105  Status              empty
//! 0x0106  WaitOutput          write position that the daemon already has (8)
//! 0x0107  Redraw              empty; since protocol version 1.1
//! 0x0108  ProgramReports      empty; since protocol version 1.2
//! 0x0109  WaitProgramReports  revision that the daemon already has (8);
//!                             since protocol version 1.2
//! 0x010A  JournalRead         position of the first byte (8), largest number
//!                             of bytes (4); since protocol version 1.3
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
const REDRAW: u16 = 0x0107;
const PROGRAM_REPORTS: u16 = 0x0108;
const WAIT_PROGRAM_REPORTS: u16 = 0x0109;
const JOURNAL_READ: u16 = 0x010A;

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

/// A message from the daemon to the holder. A later minor version of the
/// protocol adds requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
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
    /// Give the redraw sequence of the current screen and the position of
    /// the ring buffer that the screen corresponds to. Since protocol version
    /// 1.1.
    Redraw,
    /// Give the program reports: the program status records, the input line
    /// state and the last notifications. Since protocol version 1.2.
    ProgramReports,
    /// Answer when the revision of the program reports is larger than
    /// `after`, or when the child command has ended. Since protocol version
    /// 1.2.
    WaitProgramReports {
        /// The revision that the daemon already has.
        after: u64,
    },
    /// Give output bytes of the session from the journal: the bytes that
    /// the ring buffer has no more. Since protocol version 1.3.
    JournalRead {
        /// The position of the first byte that the daemon asks for.
        from: u64,
        /// The largest number of bytes of the reply. The holder gives
        /// [`journal::MAX_RANGE_LENGTH`](crate::journal::MAX_RANGE_LENGTH)
        /// bytes at most. Zero is not valid.
        max_length: u32,
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
        let mut fields = [0u8; 12];
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
                fields[..8].copy_from_slice(&after.to_le_bytes());
                (WAIT_OUTPUT, &fields[..8])
            }
            Self::Redraw => (REDRAW, &[]),
            Self::ProgramReports => (PROGRAM_REPORTS, &[]),
            Self::WaitProgramReports { after } => {
                fields[..8].copy_from_slice(&after.to_le_bytes());
                (WAIT_PROGRAM_REPORTS, &fields[..8])
            }
            Self::JournalRead { from, max_length } => {
                fields[..8].copy_from_slice(&from.to_le_bytes());
                fields[8..].copy_from_slice(&max_length.to_le_bytes());
                (JOURNAL_READ, &fields)
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
            REDRAW => Ok(Self::Redraw),
            PROGRAM_REPORTS => Ok(Self::ProgramReports),
            WAIT_PROGRAM_REPORTS => Ok(Self::WaitProgramReports {
                after: reader.u64()?,
            }),
            JOURNAL_READ => {
                let from = reader.u64()?;
                let max_length = reader.u32()?;
                if max_length == 0 {
                    return Err(DecodeError::InvalidPayload);
                }
                Ok(Self::JournalRead { from, max_length })
            }
            kind => Err(DecodeError::UnknownKind(kind)),
        }
    }

    /// The minor version of protocol version 1 that added the request.
    pub(super) fn minor_version(&self) -> u16 {
        match self {
            Self::AcquireLease
            | Self::Input(_)
            | Self::Resize(_)
            | Self::Signal(_)
            | Self::Status
            | Self::WaitOutput { .. } => 0,
            Self::Redraw => 1,
            Self::ProgramReports | Self::WaitProgramReports { .. } => 2,
            Self::JournalRead { .. } => 3,
        }
    }
}
