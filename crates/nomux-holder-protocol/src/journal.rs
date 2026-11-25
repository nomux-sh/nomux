//! The journal: the output of a session on disk.
//!
//! A holder appends the raw output bytes of its session to the journal, and
//! writes a checkpoint of the screen at intervals. The journal is in the
//! session state directory ([`session_state`](crate::session_state)), thus
//! it stays after the holder ends and after the host starts again.
//! [`JournalReader`] reads it without the holder. `docs/holder.md` has the
//! rules of the writer: when it writes, what a power cut can lose and how it
//! limits the size.
//!
//! # Positions and epochs
//!
//! A position is the number of bytes that the command of the session wrote
//! before a byte, as in the [ring buffer](crate::ring). One holder gives one
//! continuous numbering: an **epoch**. A new holder for a session that
//! already has a journal starts at position 0 with the next epoch.
//!
//! # Files
//!
//! The journal directory has one file for each segment. The name of a file
//! is [`SegmentName::file_name`]: the epoch and the position of the first
//! byte of the segment. All integers in a file are little-endian.
//!
//! ```text
//! segment header, 32 bytes
//! offset  size  field
//!      0     8  magic: "NMXJRNL\0"
//!      8     4  format version: 1
//!     12     4  header length: where the first record starts (32)
//!     16     4  epoch
//!     20     4  reserved, zero
//!     24     8  start position
//!
//! record header, 32 bytes; the payload follows it
//! offset  size  field
//!      0     1  kind: 1 output, 2 checkpoint
//!      1     1  codec of the payload: 0 stored, 1 LZ4 block
//!      2     2  rows of the window (checkpoint), or zero
//!      4     2  columns of the window (checkpoint), or zero
//!      6     2  reserved, zero
//!      8     8  position
//!     16     4  stored length: the bytes of the payload in the file
//!     20     4  raw length: the bytes of the payload after decompression
//!     24     4  CRC-32 of the payload as it is in the file
//!     28     4  CRC-32 of the bytes 0 to 27 of this header
//! ```
//!
//! - An **output** record has output bytes. Its position is the position of
//!   its first byte. The output records of a segment follow each other
//!   without a gap, and the first one starts at the start position of the
//!   segment.
//! - A **checkpoint** record has the redraw sequence of the screen and the
//!   size of the window. Its position is the position of the next output
//!   byte in the file. The redraw sequence has the effect of each byte
//!   before that position and of no byte from that position on. The position
//!   is never in a sequence that is not complete.
//!
//! Thus a reader makes the screen at a position `p` in three steps: it finds
//! the last checkpoint that is not after `p`
//! ([`JournalReader::checkpoint_before`]), gives its redraw sequence to a
//! new terminal of the size of the checkpoint, and then gives that terminal
//! the output bytes from the position of the checkpoint to `p`
//! ([`JournalReader::read`]).
//!
//! # Damaged files
//!
//! A file can end in the middle of a record, and its bytes can be wrong:
//! the holder can be killed during a write, and a different process can
//! change the files. The reader trusts no field. It uses the records of a
//! segment up to the first one that is not valid, and it continues with the
//! next segment. The caller sees the missing bytes as a range that starts
//! after the position that it asked for.

mod checksum;
mod compression;
mod reader;
mod record;
mod record_encoder;
mod segment_files;
mod segment_header;
mod segment_name;
mod segment_reader;

pub use reader::{Checkpoint, JournalRange, JournalReader};
pub use record_encoder::RecordEncoder;
pub use segment_files::{SegmentFile, segment_files};
pub use segment_header::SegmentHeader;
pub use segment_name::SegmentName;

/// The epoch of the first holder of a session. The value 0 is not an epoch.
pub const FIRST_EPOCH: u32 = 1;

/// The largest number of output bytes in one output record.
pub const MAX_OUTPUT_LENGTH: usize = 128 * 1024;
