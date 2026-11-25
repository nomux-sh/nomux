//! Makes the bytes of a record: the holder appends them to a segment file.

use super::checksum::crc32;
use super::compression::compress;
use super::record::{Codec, RecordHeader, RecordKind};
use crate::wire::WindowSize;

/// Encodes the records of a journal. It keeps one buffer for the record,
/// thus the holder does not allocate one for each record.
#[derive(Debug, Default)]
pub struct RecordEncoder {
    record: Vec<u8>,
}

impl RecordEncoder {
    /// Makes an encoder.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The bytes of an output record: `output` is the output of the session
    /// from `position` on.
    ///
    /// # Panics
    ///
    /// Panics if `output` is empty or longer than
    /// [`MAX_OUTPUT_LENGTH`](super::MAX_OUTPUT_LENGTH).
    pub fn output(&mut self, position: u64, output: &[u8]) -> &[u8] {
        self.encode(RecordKind::Output, position, output)
    }

    /// The bytes of a checkpoint record: `redraw_sequence` makes the screen
    /// that has the effect of each output byte before `position`, in a
    /// window of `size`.
    ///
    /// # Panics
    ///
    /// Panics if `redraw_sequence` is empty or longer than
    /// [`redraw::MAX_LENGTH`](crate::redraw::MAX_LENGTH), or if `size` has no
    /// cell.
    pub fn checkpoint(&mut self, position: u64, size: WindowSize, redraw_sequence: &[u8]) -> &[u8] {
        assert!(size.cells() != 0, "the window of a checkpoint has cells");
        self.encode(RecordKind::Checkpoint(size), position, redraw_sequence)
    }

    fn encode(&mut self, kind: RecordKind, position: u64, raw: &[u8]) -> &[u8] {
        assert!(
            !raw.is_empty() && raw.len() <= RecordHeader::max_raw_length(kind),
            "a record payload of {} bytes is not permitted",
            raw.len()
        );
        self.record.clear();
        self.record.resize(RecordHeader::LENGTH, 0);
        let codec = self.append_payload(raw);
        let payload = &self.record[RecordHeader::LENGTH..];
        let header = RecordHeader {
            kind,
            codec,
            position,
            stored_length: length_field(payload.len()),
            raw_length: length_field(raw.len()),
            payload_checksum: crc32(payload),
        };
        self.record[..RecordHeader::LENGTH].copy_from_slice(&header.encode());
        &self.record
    }

    /// Appends the payload in the shorter of its two forms.
    fn append_payload(&mut self, raw: &[u8]) -> Codec {
        compress(raw, &mut self.record);
        if self.record.len() - RecordHeader::LENGTH < raw.len() {
            return Codec::Lz4;
        }
        self.record.truncate(RecordHeader::LENGTH);
        self.record.extend_from_slice(raw);
        Codec::Stored
    }
}

fn length_field(length: usize) -> u32 {
    u32::try_from(length).expect("the length of a payload fits in 32 bits")
}
