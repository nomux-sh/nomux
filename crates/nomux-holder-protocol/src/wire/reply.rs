//! The replies: the messages from the holder to the daemon after the hello.
//!
//! ```text
//! kind    reply                  payload
//! 0x0002  Done                   empty
//! 0x0003  Error                  error code (2)
//! 0x0004  Status                 state (1): 0 running, 1 exited, 2 killed by
//!                                a signal; 3 bytes of zero; exit code or
//!                                signal number (4)
//! 0x0005  Output                 write position of the ring buffer (8)
//! 0x0006  Redraw                 position of the ring buffer (8), length of
//!                                the redraw sequence (4); since protocol
//!                                version 1.1
//! 0x0007  ProgramReports         revision (8), length of the encoded
//!                                program reports (4); since protocol
//!                                version 1.2
//! 0x0008  ProgramReportsChanged  revision (8); since protocol version 1.2
//! ```

use super::frame::Frame;
use super::payload::{DecodeError, PayloadReader};

const DONE: u16 = 0x0002;
const ERROR: u16 = 0x0003;
const STATUS: u16 = 0x0004;
const OUTPUT: u16 = 0x0005;
const REDRAW: u16 = 0x0006;
const PROGRAM_REPORTS: u16 = 0x0007;
const PROGRAM_REPORTS_CHANGED: u16 = 0x0008;

const STATE_RUNNING: u8 = 0;
const STATE_EXITED: u8 = 1;
const STATE_SIGNALED: u8 = 2;

/// Why the holder did not do a request. A later minor version of the
/// protocol adds codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ErrorCode {
    /// The holder does not know the request kind.
    UnknownKind,
    /// The payload of the request is not valid.
    InvalidPayload,
    /// The request needs the single-writer lease, and this connection does
    /// not have it.
    LeaseRequired,
    /// The child command has ended.
    ChildExited,
    /// This connection already has a wait request of this kind without a
    /// reply.
    WaitPending,
    /// The operating system refused the operation.
    Failed,
    /// The redraw sequence of the screen is longer than
    /// [`redraw::MAX_LENGTH`](crate::redraw::MAX_LENGTH). Since protocol
    /// version 1.1.
    RedrawTooLarge,
    /// A code from a later protocol version.
    Unrecognized(u16),
}

impl ErrorCode {
    fn encode(self) -> u16 {
        match self {
            Self::UnknownKind => 1,
            Self::InvalidPayload => 2,
            Self::LeaseRequired => 3,
            Self::ChildExited => 4,
            Self::WaitPending => 5,
            Self::Failed => 6,
            Self::RedrawTooLarge => 7,
            Self::Unrecognized(code) => code,
        }
    }

    fn decode(code: u16) -> Self {
        match code {
            1 => Self::UnknownKind,
            2 => Self::InvalidPayload,
            3 => Self::LeaseRequired,
            4 => Self::ChildExited,
            5 => Self::WaitPending,
            6 => Self::Failed,
            7 => Self::RedrawTooLarge,
            code => Self::Unrecognized(code),
        }
    }
}

impl From<DecodeError> for ErrorCode {
    fn from(error: DecodeError) -> Self {
        match error {
            DecodeError::UnknownKind(_) => Self::UnknownKind,
            DecodeError::InvalidPayload => Self::InvalidPayload,
        }
    }
}

/// The state of the child command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChildStatus {
    /// The child command runs.
    Running,
    /// The child command ended with an exit code.
    Exited {
        /// The exit code.
        code: i32,
    },
    /// A signal ended the child command.
    Signaled {
        /// The signal number of the operating system.
        signal: i32,
    },
}

/// A message from the holder that answers one request. A later minor
/// version of the protocol adds replies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Reply {
    /// The holder did the request.
    Done,
    /// The holder did not do the request.
    Error(ErrorCode),
    /// The answer to [`Request::Status`](super::Request::Status).
    Status(ChildStatus),
    /// The answer to [`Request::WaitOutput`](super::Request::WaitOutput).
    Output {
        /// The write position of the ring buffer.
        write_position: u64,
    },
    /// The answer to [`Request::Redraw`](super::Request::Redraw). It comes
    /// with a read-only descriptor of the shared memory that has the redraw
    /// sequence, as `SCM_RIGHTS` ancillary data.
    /// [`redraw::read`](crate::redraw::read) reads the memory.
    Redraw {
        /// The position of the ring buffer that the screen corresponds to.
        /// The redraw sequence has the effect of each byte before this
        /// position, and of no byte from this position on. It is the write
        /// position, or the start of the escape sequence or the UTF-8
        /// character that the output ends in.
        position: u64,
        /// The number of bytes of the redraw sequence.
        length: u32,
    },
    /// The answer to
    /// [`Request::ProgramReports`](super::Request::ProgramReports). It comes
    /// with a read-only descriptor of the shared memory that has the encoded
    /// program reports, as `SCM_RIGHTS` ancillary data.
    /// [`program_reports::read`](crate::program_reports::read) reads the
    /// memory. Since protocol version 1.2.
    ProgramReports {
        /// The revision of the program reports in the memory.
        revision: u64,
        /// The number of bytes of the encoded program reports.
        length: u32,
    },
    /// The answer to
    /// [`Request::WaitProgramReports`](super::Request::WaitProgramReports).
    /// Since protocol version 1.2.
    ProgramReportsChanged {
        /// The revision of the program reports now.
        revision: u64,
    },
}

impl Reply {
    /// Appends the encoded reply frame to `output`.
    pub fn encode(&self, request_id: u32, output: &mut Vec<u8>) {
        let mut fields = [0u8; 12];
        let (kind, payload): (u16, &[u8]) = match *self {
            Self::Done => (DONE, &[]),
            Self::Error(code) => {
                fields[..2].copy_from_slice(&code.encode().to_le_bytes());
                (ERROR, &fields[..2])
            }
            Self::Status(status) => {
                let (state, value) = match status {
                    ChildStatus::Running => (STATE_RUNNING, 0),
                    ChildStatus::Exited { code } => (STATE_EXITED, code),
                    ChildStatus::Signaled { signal } => (STATE_SIGNALED, signal),
                };
                fields[0] = state;
                fields[4..8].copy_from_slice(&value.to_le_bytes());
                (STATUS, &fields[..8])
            }
            Self::Output { write_position } => {
                fields[..8].copy_from_slice(&write_position.to_le_bytes());
                (OUTPUT, &fields[..8])
            }
            Self::Redraw { position, length } => {
                fields[..8].copy_from_slice(&position.to_le_bytes());
                fields[8..].copy_from_slice(&length.to_le_bytes());
                (REDRAW, &fields)
            }
            Self::ProgramReports { revision, length } => {
                fields[..8].copy_from_slice(&revision.to_le_bytes());
                fields[8..].copy_from_slice(&length.to_le_bytes());
                (PROGRAM_REPORTS, &fields)
            }
            Self::ProgramReportsChanged { revision } => {
                fields[..8].copy_from_slice(&revision.to_le_bytes());
                (PROGRAM_REPORTS_CHANGED, &fields[..8])
            }
        };
        let frame = Frame {
            kind,
            request_id,
            payload,
        };
        frame.encode(output);
    }

    /// Decodes a reply frame.
    ///
    /// # Errors
    ///
    /// Returns an error if this version does not know the kind, or if the
    /// payload is not valid for the kind.
    pub fn decode(frame: Frame<'_>) -> Result<Self, DecodeError> {
        let mut reader = PayloadReader::new(frame.payload);
        match frame.kind {
            DONE => Ok(Self::Done),
            ERROR => Ok(Self::Error(ErrorCode::decode(reader.u16()?))),
            STATUS => {
                let state = reader.u8()?;
                reader.bytes(3)?;
                let value = reader.i32()?;
                match state {
                    STATE_RUNNING => Ok(Self::Status(ChildStatus::Running)),
                    STATE_EXITED => Ok(Self::Status(ChildStatus::Exited { code: value })),
                    STATE_SIGNALED => Ok(Self::Status(ChildStatus::Signaled { signal: value })),
                    _ => Err(DecodeError::InvalidPayload),
                }
            }
            OUTPUT => Ok(Self::Output {
                write_position: reader.u64()?,
            }),
            REDRAW => Ok(Self::Redraw {
                position: reader.u64()?,
                length: reader.u32()?,
            }),
            PROGRAM_REPORTS => Ok(Self::ProgramReports {
                revision: reader.u64()?,
                length: reader.u32()?,
            }),
            PROGRAM_REPORTS_CHANGED => Ok(Self::ProgramReportsChanged {
                revision: reader.u64()?,
            }),
            kind => Err(DecodeError::UnknownKind(kind)),
        }
    }
}
