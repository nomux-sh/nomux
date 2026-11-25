//! Reads the journal of a session: the output bytes of a range of positions
//! and the checkpoints.

use std::io;
use std::path::Path;

use super::record::RecordKind;
use super::segment_files::{SegmentFile, segment_files};
use super::segment_reader::SegmentReader;
use crate::wire::WindowSize;

/// Output bytes of a session from the journal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalRange {
    /// The position of the first byte of `bytes`. It is the position that
    /// the caller asked for, or a later one if the journal does not have the
    /// bytes between the two.
    pub position: u64,
    /// The output bytes from `position` on. They are not empty.
    pub bytes: Vec<u8>,
}

/// The screen of a session at one position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checkpoint {
    /// The position that the screen corresponds to: the redraw sequence has
    /// the effect of each output byte before it, and of no byte from it on.
    pub position: u64,
    /// The size of the window at that position.
    pub size: WindowSize,
    /// The redraw sequence of the screen.
    pub redraw_sequence: Vec<u8>,
}

/// Reads the journal of one session. The holder is not necessary: the
/// daemon reads the journal of a session that does not run.
///
/// The reader has the list of the segment files from the moment of
/// [`open`](Self::open). A holder that runs adds and removes segments after
/// that moment.
#[derive(Debug)]
pub struct JournalReader {
    /// In the order of the output.
    segments: Vec<SegmentFile>,
}

impl JournalReader {
    /// Lists the segments of `journal_directory`. A directory that does not
    /// exist is an empty journal.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be read.
    pub fn open(journal_directory: &Path) -> io::Result<Self> {
        Ok(Self {
            segments: segment_files(journal_directory)?,
        })
    }

    /// The epochs that have a segment, in increasing order.
    #[must_use]
    pub fn epochs(&self) -> Vec<u32> {
        let mut epochs: Vec<u32> = self.segments.iter().map(|file| file.name.epoch).collect();
        epochs.dedup();
        epochs
    }

    /// Reads output bytes of `epoch` from the position `from` on:
    /// `max_length` bytes at most, and fewer at the end of a segment. The
    /// caller asks again from the end of the range for the bytes after it.
    ///
    /// The range starts after `from` if the journal does not have the byte
    /// at `from`: the oldest segments were removed, or a file is damaged.
    /// Returns `None` if the journal has no byte at `from` or after it.
    ///
    /// # Errors
    ///
    /// Returns an error if a file cannot be read. A file that is damaged is
    /// not an error.
    pub fn read(
        &self,
        epoch: u32,
        from: u64,
        max_length: usize,
    ) -> io::Result<Option<JournalRange>> {
        let segments = self.segments_of(epoch);
        let after = segments.partition_point(|file| file.name.start <= from);
        for segment in &segments[after.saturating_sub(1)..] {
            let Some(mut reader) = SegmentReader::open(segment)? else {
                continue;
            };
            let range = read_segment(&mut reader, from, max_length)?;
            if range.is_some() {
                return Ok(range);
            }
        }
        Ok(None)
    }

    /// The last checkpoint of `epoch` whose position is not after
    /// `position`. Returns `None` if the journal has no such checkpoint.
    ///
    /// To make the screen at `position`, give the redraw sequence to a new
    /// terminal of the size of the checkpoint, then give it the bytes from
    /// the position of the checkpoint to `position`. If [`read`](Self::read)
    /// gives a range that starts after the position that the caller asked
    /// for, bytes are missing and the screen cannot be made.
    ///
    /// # Errors
    ///
    /// Returns an error if a file cannot be read. A file that is damaged is
    /// not an error.
    pub fn checkpoint_before(&self, epoch: u32, position: u64) -> io::Result<Option<Checkpoint>> {
        let segments = self.segments_of(epoch);
        let after = segments.partition_point(|file| file.name.start <= position);
        for segment in segments[..after].iter().rev() {
            let Some(mut reader) = SegmentReader::open(segment)? else {
                continue;
            };
            let checkpoint = last_checkpoint(&mut reader, position)?;
            if checkpoint.is_some() {
                return Ok(checkpoint);
            }
        }
        Ok(None)
    }

    fn segments_of(&self, epoch: u32) -> &[SegmentFile] {
        let start = self
            .segments
            .partition_point(|file| file.name.epoch < epoch);
        let end = self
            .segments
            .partition_point(|file| file.name.epoch <= epoch);
        &self.segments[start..end]
    }
}

/// Reads the output bytes of one segment from `from` on.
fn read_segment(
    reader: &mut SegmentReader,
    from: u64,
    max_length: usize,
) -> io::Result<Option<JournalRange>> {
    let mut range: Option<JournalRange> = None;
    while let Some(header) = reader.next_header()? {
        let end = header.position.saturating_add(u64::from(header.raw_length));
        if header.kind != RecordKind::Output || end <= from {
            reader.skip_payload(&header)?;
            continue;
        }
        let Some(output) = reader.payload(&header)? else {
            break;
        };
        let range = range.get_or_insert_with(|| JournalRange {
            position: header.position.max(from),
            bytes: Vec::new(),
        });
        // Only the first record of the range starts before `from`.
        let skipped = usize::try_from(from.saturating_sub(header.position)).unwrap_or(usize::MAX);
        let wanted = max_length - range.bytes.len();
        range.bytes.extend(output.iter().skip(skipped).take(wanted));
        if range.bytes.len() >= max_length {
            break;
        }
    }
    Ok(range.filter(|range| !range.bytes.is_empty()))
}

/// The last checkpoint of one segment whose position is not after
/// `position`.
fn last_checkpoint(reader: &mut SegmentReader, position: u64) -> io::Result<Option<Checkpoint>> {
    let mut last = None;
    while let Some(header) = reader.next_header()? {
        if header.position > position {
            break;
        }
        let RecordKind::Checkpoint(size) = header.kind else {
            reader.skip_payload(&header)?;
            continue;
        };
        let Some(redraw_sequence) = reader.payload(&header)? else {
            break;
        };
        last = Some(Checkpoint {
            position: header.position,
            size,
            redraw_sequence,
        });
    }
    Ok(last)
}
