//! The header of a segment file.

use super::segment_name::SegmentName;

const MAGIC: [u8; 8] = *b"NMXJRNL\0";
const FORMAT_VERSION: u32 = 1;
const HEADER_LENGTH: u32 = 32;
/// A later version can make the header longer. A header of more than this
/// length is not from a writer.
const MAX_HEADER_LENGTH: u32 = 4096;

/// The first bytes of a segment file. They say the same as the name of the
/// file, thus a file that was renamed or copied is found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SegmentHeader {
    /// The epoch and the start position of the segment.
    pub name: SegmentName,
}

impl SegmentHeader {
    /// The number of bytes of the header in this format version.
    pub const LENGTH: usize = HEADER_LENGTH as usize;

    /// The bytes of the header.
    #[must_use]
    pub fn encode(self) -> [u8; Self::LENGTH] {
        let mut bytes = [0; Self::LENGTH];
        bytes[..8].copy_from_slice(&MAGIC);
        bytes[8..12].copy_from_slice(&FORMAT_VERSION.to_le_bytes());
        bytes[12..16].copy_from_slice(&HEADER_LENGTH.to_le_bytes());
        bytes[16..20].copy_from_slice(&self.name.epoch.to_le_bytes());
        bytes[24..32].copy_from_slice(&self.name.start.to_le_bytes());
        bytes
    }

    /// Reads the first bytes of a file. Returns the header and the offset of
    /// the first record, or `None` if the bytes are not the header of a
    /// format version that this crate reads.
    pub(super) fn decode(bytes: &[u8; Self::LENGTH]) -> Option<(Self, u64)> {
        let field = |offset: usize| {
            u32::from_le_bytes([
                bytes[offset],
                bytes[offset + 1],
                bytes[offset + 2],
                bytes[offset + 3],
            ])
        };
        let header_length = field(12);
        let is_valid = bytes[..8] == MAGIC
            && field(8) == FORMAT_VERSION
            && (HEADER_LENGTH..=MAX_HEADER_LENGTH).contains(&header_length);
        if !is_valid {
            return None;
        }
        let mut start = [0; 8];
        start.copy_from_slice(&bytes[24..32]);
        let name = SegmentName {
            epoch: field(16),
            start: u64::from_le_bytes(start),
        };
        Some((Self { name }, u64::from(header_length)))
    }
}
