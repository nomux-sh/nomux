//! The program reports: what the programs and the shell of a session
//! reported in the terminal output.
//!
//! A holder reads the reports from the output and keeps the last state:
//!
//! - the program status records (OSC 7501), one for each `id`;
//! - the input line state (the prompt marks of OSC 133);
//! - the last notifications (OSC 9 and OSC 777).
//!
//! The holder gives each change of this state a number, the revision. The
//! daemon reads the state with
//! [`Request::ProgramReports`](crate::wire::Request::ProgramReports) and
//! waits for a larger revision with
//! [`Request::WaitProgramReports`](crate::wire::Request::WaitProgramReports).
//!
//! The encoded program reports can be longer than the payload of a frame.
//! Thus they go to the daemon in new shared memory, as a redraw sequence
//! does: the reply has their length and comes with a read-only descriptor.
//!
//! # Encoding
//!
//! All integers are little-endian. A block is a length (2) and that number
//! of bytes. A text is a block of UTF-8. A text that is absent is empty.
//!
//! ```text
//! offset  size  field
//!      0     4  header length: the number of bytes of the header after
//!               this field (28)
//!      4     8  revision of the program reports
//!     12     1  last prompt mark: 0 none, 1 prompt start, 2 input start,
//!               3 command start, 4 command end
//!     13     1  flags: bit 0 input after the mark, bit 1 the exit code is
//!               present
//!     14     2  zero
//!     16     4  exit code of the last command (signed)
//!     20     8  revision of the input line state
//!     28     2  number of program status records
//!     30     2  number of notifications
//!     32        one block for each program status record, then one block
//!               for each notification
//! ```
//!
//! A program status record:
//!
//! ```text
//!   8  revision of the record
//!   1  state: 1 idle, 2 working, 3 done, 4 blocked, 5 error
//!   1  kind: 0 none, 1 permission, 2 question, 3 auth
//!   1  progress: 0 to 100, or 255 for none
//!   1  zero
//!      four texts: id, app, title, message
//! ```
//!
//! A notification:
//!
//! ```text
//!   8  revision of the notification
//!      two texts: title, body
//! ```
//!
//! A later minor version of the protocol adds fields at the end of the
//! header or at the end of a block. A reader uses the lengths to ignore the
//! bytes that it does not know. It ignores a record whose state it does not
//! know, and it reads a kind or a prompt mark that it does not know as none.

mod input_line;
mod notification;
mod program_status;

use std::io;
use std::os::fd::AsFd;

pub use input_line::{InputLine, PromptMark};
pub use notification::Notification;
pub use program_status::{BlockedKind, ProgramState, ProgramStatus};

use crate::shared_bytes;
use crate::wire::DecodeError;
use crate::wire::payload::PayloadReader;

/// The largest number of bytes of the encoded program reports.
pub const MAX_LENGTH: usize = 256 * 1024;

const HEADER_LENGTH: u32 = 28;

/// The state that a holder keeps from the reports in the terminal output.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProgramReports {
    /// The number of changes that the holder made to this state.
    pub revision: u64,
    /// The input line state.
    pub input_line: InputLine,
    /// The program status records. No two records have the same `id`.
    pub statuses: Vec<ProgramStatus>,
    /// The last notifications. The oldest one is first.
    pub notifications: Vec<Notification>,
}

impl ProgramReports {
    /// Appends the encoded program reports to `output`.
    ///
    /// # Panics
    ///
    /// Panics if there are more than 65535 records or notifications, or if
    /// one of them is longer than 65535 bytes. A holder keeps less.
    pub fn encode(&self, output: &mut Vec<u8>) {
        let count = |length: usize| u16::try_from(length).expect("the holder keeps fewer items");
        output.extend_from_slice(&HEADER_LENGTH.to_le_bytes());
        output.extend_from_slice(&self.revision.to_le_bytes());
        self.input_line.encode(output);
        output.extend_from_slice(&count(self.statuses.len()).to_le_bytes());
        output.extend_from_slice(&count(self.notifications.len()).to_le_bytes());
        for status in &self.statuses {
            status.encode(output);
        }
        for notification in &self.notifications {
            notification.encode(output);
        }
    }

    /// Decodes the program reports.
    ///
    /// # Errors
    ///
    /// Returns [`DecodeError::InvalidPayload`] if `bytes` end before the
    /// last field, or if a text is not UTF-8.
    pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = PayloadReader::new(bytes);
        let header_length =
            usize::try_from(reader.u32()?).map_err(|_| DecodeError::InvalidPayload)?;
        let mut header = PayloadReader::new(reader.bytes(header_length)?);
        let revision = header.u64()?;
        let input_line = InputLine::decode(&mut header)?;
        let status_count = header.u16()?;
        let notification_count = header.u16()?;

        let mut statuses = Vec::new();
        for _ in 0..status_count {
            statuses.extend(ProgramStatus::decode(&mut reader.block()?)?);
        }
        let mut notifications = Vec::new();
        for _ in 0..notification_count {
            notifications.push(Notification::decode(&mut reader.block()?)?);
        }
        Ok(Self {
            revision,
            input_line,
            statuses,
            notifications,
        })
    }
}

/// Writes the encoded program reports at the start of `memory`. The holder
/// calls this.
///
/// `memory` must be shared memory that is at least `encoded.len()` bytes
/// long.
///
/// # Errors
///
/// Returns the error of `mmap`.
pub fn write(memory: impl AsFd, encoded: &[u8]) -> io::Result<()> {
    shared_bytes::write(memory.as_fd(), encoded)
}

/// Reads the program reports of `length` encoded bytes out of the memory
/// that came with a
/// [`Reply::ProgramReports`](crate::wire::Reply::ProgramReports). The daemon
/// calls this.
///
/// # Errors
///
/// Returns an error if `length` is larger than [`MAX_LENGTH`] or than the
/// memory, if `fstat` or `mmap` fails, or if the bytes are not valid.
pub fn read(memory: impl AsFd, length: u32) -> io::Result<ProgramReports> {
    let encoded = shared_bytes::read(memory.as_fd(), length, MAX_LENGTH)?;
    ProgramReports::decode(&encoded)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

/// A text that is absent is empty in the encoding.
fn optional_text(text: String) -> Option<String> {
    (!text.is_empty()).then_some(text)
}
