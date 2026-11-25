//! A journal directory that a script wrote, and what a reader must find in
//! it: the `journal_reader` target compares the answers of the reader with
//! this.
//!
//! The output of an epoch is a function of the position, thus each byte
//! that a reader returns has one correct value.

mod segment;

use std::path::Path;

use nomux_holder_protocol::journal::{Checkpoint, FIRST_EPOCH, RecordEncoder, SegmentName};
use nomux_holder_protocol::wire::WindowSize;
use tempfile::TempDir;

use self::segment::{Content, WrittenSegment};
use crate::forged_record;
use crate::journal_script::{Change, Position};

/// The output changes its kind after each block of this number of bytes:
/// LZ4 makes one kind shorter, and it stores the other kind as it is.
const OUTPUT_BLOCK: u64 = 256;

/// A prime number, thus the output that LZ4 makes shorter is different at
/// the same offset of two blocks.
const OUTPUT_PERIOD: u64 = 251;

/// The journal directory of one script.
#[derive(Debug)]
pub struct WrittenJournal {
    directory: TempDir,
    /// In the order in which the script made them.
    segments: Vec<WrittenSegment>,
    /// The index of the segment that gets the next record.
    open: usize,
    /// The position of the next output byte of the writer.
    position: u64,
    encoder: RecordEncoder,
}

impl Default for WrittenJournal {
    fn default() -> Self {
        Self::new()
    }
}

impl WrittenJournal {
    /// The journal of a holder that wrote no record: the first segment of
    /// the first epoch.
    ///
    /// # Panics
    ///
    /// Panics, as each method that writes, if the host has no space for the
    /// files.
    #[must_use]
    pub fn new() -> Self {
        let directory = tempfile::tempdir().expect("the host has a directory for temporary files");
        let name = SegmentName {
            epoch: FIRST_EPOCH,
            start: 0,
        };
        let first = WrittenSegment::create(directory.path(), name);
        Self {
            directory,
            segments: vec![first],
            open: 0,
            position: 0,
            encoder: RecordEncoder::new(),
        }
    }

    /// The journal directory.
    #[must_use]
    pub fn directory(&self) -> &Path {
        self.directory.path()
    }

    /// Makes one change to the files.
    pub fn apply(&mut self, change: &Change<'_>) {
        match *change {
            Change::Output { length } => self.write_output(length),
            Change::Checkpoint {
                size,
                length,
                compressible,
            } => self.write_checkpoint(size, length, compressible),
            Change::NextSegment { gap } => {
                self.start_segment(self.open_name().epoch, self.position.saturating_add(gap));
            }
            Change::SegmentBeforeLargestPosition { distance } => {
                self.start_segment(self.open_name().epoch, u64::MAX - distance);
            }
            Change::NextEpoch => self.start_segment(self.open_name().epoch + 1, 0),
            Change::ForgedRecord(record) => {
                let open = &mut self.segments[self.open];
                open.append_forged(&forged_record::encode(&record, open.next_position));
            }
            Change::RawSegment { start, content } => {
                let name = SegmentName {
                    epoch: self.open_name().epoch,
                    start: self.position_of(start),
                };
                if !self.has_segment(name) {
                    let segment = WrittenSegment::forged(self.directory.path(), name, content);
                    self.segments.push(segment);
                }
            }
            Change::CopySegment {
                segment,
                epoch,
                start,
            } => {
                let name = SegmentName {
                    epoch: self.epoch_of(epoch),
                    start: self.position_of(start),
                };
                if !self.has_segment(name) {
                    let source = self.selected_segment(segment);
                    let copy = WrittenSegment::copy(self.directory.path(), name, source);
                    self.segments.push(copy);
                }
            }
            Change::FlipBits {
                segment,
                offset,
                mask,
            } => self.selected_segment_mut(segment).flip_bits(offset, mask),
            Change::Truncate { segment, length } => {
                self.selected_segment_mut(segment).truncate(length);
            }
            Change::CutAfterRecord { segment, record } => {
                self.selected_segment_mut(segment).cut_after_record(record);
            }
        }
    }

    /// The position that a step selects.
    #[must_use]
    pub fn position_of(&self, position: Position) -> u64 {
        match position {
            Position::Absolute(position) => position,
            Position::AfterOpenSegmentStart(offset) => {
                self.open_name().start.saturating_add(offset)
            }
            Position::BeforeLargest(distance) => u64::MAX - distance,
        }
    }

    /// The epoch that a step selects: an epoch of the journal, or one of two
    /// epochs that it does not have.
    #[must_use]
    pub fn epoch_of(&self, selector: u8) -> u32 {
        let mut epochs = self.epochs();
        epochs.extend([0, self.open_name().epoch + 1]);
        epochs[usize::from(selector) % epochs.len()]
    }

    /// The epochs that have a segment file, in increasing order.
    #[must_use]
    pub fn epochs(&self) -> Vec<u32> {
        let mut epochs: Vec<u32> = self.names().map(|name| name.epoch).collect();
        epochs.sort_unstable();
        epochs.dedup();
        epochs
    }

    /// The name of each segment file.
    pub fn names(&self) -> impl Iterator<Item = SegmentName> {
        self.segments.iter().map(|segment| segment.name)
    }

    /// `true` if a hostile writer made bytes of `epoch`. A reader can then
    /// return bytes that are not the output of the epoch.
    #[must_use]
    pub fn is_forged(&self, epoch: u32) -> bool {
        self.segments_of(epoch).any(|segment| segment.forged)
    }

    /// The position up to which a reader must return the output from `from`
    /// on without a gap. `None` if the journal does not need to have the
    /// byte at `from`.
    ///
    /// The segment of a position is the last one that does not start after
    /// it. The reader must find the records of the writer in that segment
    /// that are before the first byte that the writer did not make.
    #[must_use]
    pub fn trusted_output_end(&self, epoch: u32, from: u64) -> Option<u64> {
        self.segments_of(epoch)
            .filter(|segment| segment.name.start <= from)
            .max_by_key(|segment| segment.name.start)
            .map(WrittenSegment::trusted_output_end)
            .filter(|end| *end > from)
    }

    /// The largest position of the checkpoints of `epoch` that a reader must
    /// find and that are not after `position`.
    #[must_use]
    pub fn last_trusted_checkpoint(&self, epoch: u32, position: u64) -> Option<u64> {
        self.segments_of(epoch)
            .flat_map(WrittenSegment::trusted_checkpoints)
            .map(|checkpoint| checkpoint.position)
            .filter(|at| *at <= position)
            .max()
    }

    /// `true` if the writer wrote `checkpoint` in `epoch`.
    #[must_use]
    pub fn wrote_checkpoint(&self, epoch: u32, checkpoint: &Checkpoint) -> bool {
        self.segments_of(epoch)
            .flat_map(WrittenSegment::written_checkpoints)
            .any(|written| written == checkpoint)
    }

    fn write_output(&mut self, length: usize) {
        let Some(end) = self.position.checked_add(length as u64) else {
            return;
        };
        let epoch = self.open_name().epoch;
        let output: Vec<u8> = (self.position..end)
            .map(|position| output_byte(epoch, position))
            .collect();
        let record = self.encoder.output(self.position, &output);
        self.segments[self.open].append(record, Content::OutputTo(end));
        self.position = end;
    }

    fn write_checkpoint(&mut self, size: WindowSize, length: usize, compressible: bool) {
        let redraw_sequence: Vec<u8> = (0..length as u64)
            .map(|offset| {
                if compressible {
                    patterned(offset)
                } else {
                    mixed(self.position ^ offset)
                }
            })
            .collect();
        let record = self
            .encoder
            .checkpoint(self.position, size, &redraw_sequence);
        let checkpoint = Checkpoint {
            position: self.position,
            size,
            redraw_sequence,
        };
        self.segments[self.open].append(record, Content::Checkpoint(checkpoint));
    }

    /// Starts a segment as the holder does: after the output that it wrote,
    /// and with a name that no file has.
    fn start_segment(&mut self, epoch: u32, start: u64) {
        let name = SegmentName { epoch, start };
        let is_after_output = epoch != self.open_name().epoch || start >= self.position;
        if !is_after_output || self.has_segment(name) {
            return;
        }
        self.segments
            .push(WrittenSegment::create(self.directory.path(), name));
        self.open = self.segments.len() - 1;
        self.position = start;
    }

    fn open_name(&self) -> SegmentName {
        self.segments[self.open].name
    }

    fn has_segment(&self, name: SegmentName) -> bool {
        self.names().any(|existing| existing == name)
    }

    fn selected_segment(&self, selector: u8) -> &WrittenSegment {
        &self.segments[usize::from(selector) % self.segments.len()]
    }

    fn selected_segment_mut(&mut self, selector: u8) -> &mut WrittenSegment {
        let index = usize::from(selector) % self.segments.len();
        &mut self.segments[index]
    }

    fn segments_of(&self, epoch: u32) -> impl Iterator<Item = &WrittenSegment> {
        self.segments
            .iter()
            .filter(move |segment| segment.name.epoch == epoch)
    }
}

/// The output byte of `epoch` at `position`.
#[must_use]
pub fn output_byte(epoch: u32, position: u64) -> u8 {
    let position_in_epoch = position ^ (u64::from(epoch) << 56);
    if (position / OUTPUT_BLOCK).is_multiple_of(2) {
        patterned(position_in_epoch)
    } else {
        mixed(position_in_epoch)
    }
}

/// A byte of a sequence that repeats.
fn patterned(value: u64) -> u8 {
    low_byte(value % OUTPUT_PERIOD)
}

/// A byte of a sequence that has no pattern: the finalizer of `SplitMix64`.
fn mixed(value: u64) -> u8 {
    let mut mixed = value.wrapping_add(0x9E37_79B9_7F4A_7C15);
    mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    low_byte(mixed ^ (mixed >> 31))
}

fn low_byte(value: u64) -> u8 {
    value.to_le_bytes()[0]
}
