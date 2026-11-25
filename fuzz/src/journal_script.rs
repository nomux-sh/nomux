//! The input of the `journal_reader` target: what a holder writes to a
//! journal, how the files change after that, and what a reader asks for.
//!
//! The checksums of the format stop arbitrary bytes at the first header.
//! Thus the input is a script. The target makes the valid records with the
//! encoder of the holder, and the script changes the files as a power cut, a
//! different process and a hostile writer do.
//!
//! ```text
//! offset  size  part
//!      0     1  the number of steps, modulo 65
//!      1   8*n  the steps: one byte for the kind, 7 bytes for the operands
//!   1+8n  rest  the forged bytes
//! ```
//!
//! A step of a fixed length keeps the other steps after a mutation. The
//! forged bytes are the fields and the payloads of the forged records, and
//! the content of the raw segments: a step selects a range of them.

use std::slice;

use nomux_holder_protocol::wire::WindowSize;

const MAX_STEPS: u8 = 64;
const STEP_BYTES: usize = 8;
const KINDS: u8 = 13;

/// The largest payload of a record of the writer. A short payload keeps a
/// run fast: one record of the largest length that the format permits makes
/// a run 30 times slower.
const MAX_PAYLOAD_BYTES: u16 = 512;

/// A position in the output of an epoch. Positions near the start of a
/// segment and near the largest position are where the defects are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Position {
    /// This position.
    Absolute(u64),
    /// This number of bytes after the start of the segment that the writer
    /// has open.
    AfterOpenSegmentStart(u64),
    /// This number of bytes before the largest position.
    BeforeLargest(u64),
}

/// Which fields of a forged record keep the value of the forged bytes. Each
/// other field gets the value that makes the reader continue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeptFields(u8);

impl KeptFields {
    /// If not kept: the position that the reader expects next.
    #[must_use]
    pub fn position(self) -> bool {
        self.0 & 1 != 0
    }

    /// If not kept: the length of the payload in the file.
    #[must_use]
    pub fn stored_length(self) -> bool {
        self.0 & 2 != 0
    }

    /// If not kept: the length of the payload in the file, as a stored
    /// payload has it.
    #[must_use]
    pub fn raw_length(self) -> bool {
        self.0 & 4 != 0
    }

    /// If not kept: the checksum of the payload.
    #[must_use]
    pub fn payload_checksum(self) -> bool {
        self.0 & 8 != 0
    }
}

/// A record that a hostile writer makes. The checksum of its header is
/// always correct.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ForgedRecord<'input> {
    /// The header without its checksum: 28 bytes, or fewer at the end of
    /// the forged bytes. The rest is zero.
    pub fields: &'input [u8],
    /// The payload as it is in the file.
    pub payload: &'input [u8],
    /// The fields that keep the value of `fields`.
    pub kept: KeptFields,
}

/// One change to the files of the journal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change<'input> {
    /// The holder writes an output record of this number of bytes.
    Output {
        /// 1 or more.
        length: usize,
    },
    /// The holder writes a checkpoint.
    Checkpoint {
        /// The window. It has a cell.
        size: WindowSize,
        /// The number of bytes of the redraw sequence: 1 or more.
        length: usize,
        /// `true` for a redraw sequence that LZ4 makes shorter.
        compressible: bool,
    },
    /// The holder starts a segment. `gap` output bytes before it are in no
    /// file, as after a write that failed.
    NextSegment {
        /// The number of output bytes that are missing.
        gap: u64,
    },
    /// The holder starts a segment near the largest position.
    SegmentBeforeLargestPosition {
        /// The number of positions between the start and the largest one.
        distance: u64,
    },
    /// A new holder starts the next epoch.
    NextEpoch,
    /// A hostile writer appends a record to the open segment.
    ForgedRecord(ForgedRecord<'input>),
    /// A hostile writer makes a segment file with arbitrary bytes.
    RawSegment {
        /// The start position in the name of the file.
        start: Position,
        /// The bytes of the file.
        content: &'input [u8],
    },
    /// A different process copies a segment file to the name of a different
    /// segment.
    CopySegment {
        /// Selects the segment.
        segment: u8,
        /// Selects the epoch in the name of the copy.
        epoch: u8,
        /// The start position in the name of the copy.
        start: Position,
    },
    /// Bits of one byte of a segment file change.
    FlipBits {
        /// Selects the segment.
        segment: u8,
        /// Selects the byte.
        offset: u16,
        /// The bits that change: not zero.
        mask: u8,
    },
    /// A segment file loses its end.
    Truncate {
        /// Selects the segment.
        segment: u8,
        /// Selects the new length.
        length: u16,
    },
    /// A segment file loses its end after one of the records of the holder,
    /// as after a power cut between two writes.
    CutAfterRecord {
        /// Selects the segment.
        segment: u8,
        /// Selects the record.
        record: u16,
    },
}

/// One step of a script.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step<'input> {
    /// The files change.
    Change(Change<'input>),
    /// A reader asks for output bytes.
    Read {
        /// Selects the epoch.
        epoch: u8,
        /// The position of the first byte.
        from: Position,
        /// The largest number of bytes.
        max_length: usize,
    },
    /// A reader asks for the last checkpoint that is not after a position.
    CheckpointBefore {
        /// Selects the epoch.
        epoch: u8,
        /// The position.
        position: Position,
    },
}

/// The steps of one fuzz input.
#[derive(Debug, Clone)]
pub struct JournalScript<'input> {
    steps: slice::Iter<'input, [u8; STEP_BYTES]>,
    forged: &'input [u8],
}

impl<'input> JournalScript<'input> {
    /// Reads `input` as a script.
    #[must_use]
    pub fn new(input: &'input [u8]) -> Self {
        let (count, rest) = input.split_first().unwrap_or((&0, &[]));
        let (steps, _) = rest.as_chunks::<STEP_BYTES>();
        let count = steps.len().min(usize::from(count % (MAX_STEPS + 1)));
        Self {
            steps: steps[..count].iter(),
            forged: &rest[count * STEP_BYTES..],
        }
    }

    /// The forged bytes from `offset` on: `length` bytes, or fewer at the
    /// end.
    fn forged(&self, offset: usize, length: usize) -> &'input [u8] {
        let start = self.forged.len().min(offset);
        let end = self.forged.len().min(start + length);
        &self.forged[start..end]
    }

    fn step(&self, [kind, operands @ ..]: [u8; STEP_BYTES]) -> Step<'input> {
        let word = |index: usize| u16::from_le_bytes([operands[index], operands[index + 1]]);
        let selector = operands[0];
        let change = match kind % KINDS {
            0 => Change::Output {
                length: payload_length(word(0)),
            },
            1 => Change::Checkpoint {
                size: WindowSize {
                    rows: 1 + u16::from(operands[0]),
                    columns: 1 + u16::from(operands[1]),
                },
                length: payload_length(word(2)),
                compressible: operands[4] % 2 == 0,
            },
            2 => Change::NextSegment {
                gap: u64::from(word(0)),
            },
            3 => Change::SegmentBeforeLargestPosition {
                distance: u64::from(word(0)),
            },
            4 => Change::NextEpoch,
            5 => Change::ForgedRecord(ForgedRecord {
                fields: self.forged(usize::from(word(1)), FORGED_FIELDS_BYTES),
                payload: self.forged(
                    usize::from(word(1)) + FORGED_FIELDS_BYTES,
                    usize::from(word(3)),
                ),
                kept: KeptFields(selector),
            }),
            6 => Change::RawSegment {
                start: position(selector, word(1)),
                content: self.forged(usize::from(word(3)), usize::from(word(5))),
            },
            7 => Change::FlipBits {
                segment: selector,
                offset: word(1),
                mask: operands[3].max(1),
            },
            8 => Change::Truncate {
                segment: selector,
                length: word(1),
            },
            9 => Change::CutAfterRecord {
                segment: selector,
                record: word(1),
            },
            10 => Change::CopySegment {
                segment: selector,
                epoch: operands[1],
                start: position(operands[2], word(3)),
            },
            11 => {
                let factor = if operands[6] % 2 == 0 {
                    1
                } else {
                    LONG_READ_FACTOR
                };
                return Step::Read {
                    epoch: selector,
                    from: position(operands[1], word(2)),
                    max_length: usize::from(word(4)) * factor,
                };
            }
            _ => {
                return Step::CheckpointBefore {
                    epoch: selector,
                    position: position(operands[1], word(2)),
                };
            }
        };
        Step::Change(change)
    }
}

impl<'input> Iterator for JournalScript<'input> {
    type Item = Step<'input>;

    fn next(&mut self) -> Option<Self::Item> {
        let bytes = *self.steps.next()?;
        Some(self.step(bytes))
    }
}

/// The header of a record without its checksum.
const FORGED_FIELDS_BYTES: usize = 28;

/// With this factor, a `Read` step can ask for more bytes than the largest
/// record has.
const LONG_READ_FACTOR: usize = 4;

fn payload_length(selector: u16) -> usize {
    1 + usize::from(selector % MAX_PAYLOAD_BYTES)
}

fn position(kind: u8, value: u16) -> Position {
    let value = u64::from(value);
    match kind % 3 {
        0 => Position::Absolute(value),
        1 => Position::AfterOpenSegmentStart(value),
        _ => Position::BeforeLargest(value),
    }
}
