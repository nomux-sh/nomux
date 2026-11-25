//! The header of a record in a segment file.

use super::MAX_OUTPUT_LENGTH;
use super::checksum::crc32;
use crate::redraw;
use crate::wire::WindowSize;

const KIND_OUTPUT: u8 = 1;
const KIND_CHECKPOINT: u8 = 2;
const CODEC_STORED: u8 = 0;
const CODEC_LZ4: u8 = 1;

/// The bytes of the header that its own checksum covers.
const CHECKED_LENGTH: usize = RecordHeader::LENGTH - 4;

/// What the payload of a record is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RecordKind {
    /// Output bytes of the session.
    Output,
    /// The redraw sequence of the screen in a window of this size.
    Checkpoint(WindowSize),
}

/// How the payload is in the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Codec {
    /// The bytes as they are.
    Stored,
    /// One LZ4 block.
    Lz4,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RecordHeader {
    pub(super) kind: RecordKind,
    pub(super) codec: Codec,
    pub(super) position: u64,
    pub(super) stored_length: u32,
    pub(super) raw_length: u32,
    pub(super) payload_checksum: u32,
}

impl RecordHeader {
    pub(super) const LENGTH: usize = 32;

    pub(super) fn encode(&self) -> [u8; Self::LENGTH] {
        let mut bytes = [0; Self::LENGTH];
        let size = match self.kind {
            RecordKind::Output => {
                bytes[0] = KIND_OUTPUT;
                WindowSize {
                    rows: 0,
                    columns: 0,
                }
            }
            RecordKind::Checkpoint(size) => {
                bytes[0] = KIND_CHECKPOINT;
                size
            }
        };
        bytes[1] = match self.codec {
            Codec::Stored => CODEC_STORED,
            Codec::Lz4 => CODEC_LZ4,
        };
        bytes[2..4].copy_from_slice(&size.rows.to_le_bytes());
        bytes[4..6].copy_from_slice(&size.columns.to_le_bytes());
        bytes[8..16].copy_from_slice(&self.position.to_le_bytes());
        bytes[16..20].copy_from_slice(&self.stored_length.to_le_bytes());
        bytes[20..24].copy_from_slice(&self.raw_length.to_le_bytes());
        bytes[24..28].copy_from_slice(&self.payload_checksum.to_le_bytes());
        let checksum = crc32(&bytes[..CHECKED_LENGTH]);
        bytes[CHECKED_LENGTH..].copy_from_slice(&checksum.to_le_bytes());
        bytes
    }

    /// Reads a header. Returns `None` if a field has a value that the
    /// writer does not make: the bytes are not a record.
    pub(super) fn decode(bytes: &[u8; Self::LENGTH]) -> Option<Self> {
        let u16_at = |offset: usize| u16::from_le_bytes([bytes[offset], bytes[offset + 1]]);
        let u32_at = |offset: usize| {
            u32::from_le_bytes([
                bytes[offset],
                bytes[offset + 1],
                bytes[offset + 2],
                bytes[offset + 3],
            ])
        };
        if u32_at(CHECKED_LENGTH) != crc32(&bytes[..CHECKED_LENGTH]) {
            return None;
        }
        let size = WindowSize {
            rows: u16_at(2),
            columns: u16_at(4),
        };
        let kind = match bytes[0] {
            KIND_OUTPUT => RecordKind::Output,
            KIND_CHECKPOINT if size.rows != 0 && size.columns != 0 => RecordKind::Checkpoint(size),
            _ => return None,
        };
        let codec = match bytes[1] {
            CODEC_STORED => Codec::Stored,
            CODEC_LZ4 => Codec::Lz4,
            _ => return None,
        };
        let mut position = [0; 8];
        position.copy_from_slice(&bytes[8..16]);
        let header = Self {
            kind,
            codec,
            position: u64::from_le_bytes(position),
            stored_length: u32_at(16),
            raw_length: u32_at(20),
            payload_checksum: u32_at(24),
        };
        header.has_valid_lengths().then_some(header)
    }

    /// The largest number of bytes that a payload of `kind` has after
    /// decompression.
    pub(super) fn max_raw_length(kind: RecordKind) -> usize {
        match kind {
            RecordKind::Output => MAX_OUTPUT_LENGTH,
            RecordKind::Checkpoint(_) => redraw::MAX_LENGTH,
        }
    }

    /// The writer stores a payload as it is when the LZ4 block is not
    /// shorter. Thus a payload in the file is never longer than its bytes.
    fn has_valid_lengths(&self) -> bool {
        let raw_length = self.raw_length as usize;
        let is_stored_length_valid = match self.codec {
            Codec::Stored => self.stored_length == self.raw_length,
            Codec::Lz4 => self.stored_length < self.raw_length,
        };
        raw_length != 0 && raw_length <= Self::max_raw_length(self.kind) && is_stored_length_valid
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OUTPUT: RecordHeader = RecordHeader {
        kind: RecordKind::Output,
        codec: Codec::Lz4,
        position: 1 << 40,
        stored_length: 100,
        raw_length: 300,
        payload_checksum: 0xDEAD_BEEF,
    };

    /// A header that a hostile writer makes: the checksum is correct.
    fn decode_with(change: impl FnOnce(&mut [u8; RecordHeader::LENGTH])) -> Option<RecordHeader> {
        let mut bytes = OUTPUT.encode();
        change(&mut bytes);
        let checksum = crc32(&bytes[..CHECKED_LENGTH]);
        bytes[CHECKED_LENGTH..].copy_from_slice(&checksum.to_le_bytes());
        RecordHeader::decode(&bytes)
    }

    #[test]
    fn a_header_with_a_value_that_the_writer_does_not_make_is_refused() {
        assert_eq!(decode_with(|_| {}), Some(OUTPUT));

        let too_long = (u32::try_from(MAX_OUTPUT_LENGTH).unwrap() + 1).to_le_bytes();
        assert_eq!(
            decode_with(|bytes| bytes[20..24].copy_from_slice(&too_long)),
            None
        );
        // A payload that is longer in the file than after decompression.
        assert_eq!(
            decode_with(|bytes| bytes[16..20].copy_from_slice(&301u32.to_le_bytes())),
            None
        );
        assert_eq!(decode_with(|bytes| bytes[20..24].fill(0)), None);
        assert_eq!(decode_with(|bytes| bytes[0] = 3), None);
        assert_eq!(decode_with(|bytes| bytes[1] = 2), None);
        // A checkpoint for a window without a cell.
        assert_eq!(decode_with(|bytes| bytes[0] = KIND_CHECKPOINT), None);
    }

    #[test]
    fn a_header_with_one_changed_bit_is_refused() {
        let bytes = OUTPUT.encode();
        for bit in 0..RecordHeader::LENGTH * 8 {
            let mut changed = bytes;
            changed[bit / 8] ^= 1 << (bit % 8);
            assert_eq!(RecordHeader::decode(&changed), None, "bit {bit}");
        }
    }
}
