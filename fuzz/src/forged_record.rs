//! The bytes of a record that a hostile writer appends to a segment: each
//! field can have each value, and the checksums are correct. Thus the
//! reader takes the fields, which it must not trust.
//!
//! `journal.rs` of `nomux-holder-protocol` has the layout of a record.

use crate::journal_script::ForgedRecord;

const HEADER_BYTES: usize = 32;
const POSITION_FIELD: usize = 8;
const STORED_LENGTH_FIELD: usize = 16;
const RAW_LENGTH_FIELD: usize = 20;
const PAYLOAD_CHECKSUM_FIELD: usize = 24;
const HEADER_CHECKSUM_FIELD: usize = 28;

const KIND_OUTPUT: u8 = 1;

/// The bytes of a forged record, and what a reader that takes it does.
#[derive(Debug)]
pub struct Encoded {
    /// The header and the payload.
    pub bytes: Vec<u8>,
    /// The position that the reader expects after the record, if it took
    /// the record at `next_position`.
    pub next_position: u64,
}

/// Encodes `record`. `next_position` is the position that the reader
/// expects before the record.
#[must_use]
pub fn encode(record: &ForgedRecord<'_>, next_position: u64) -> Encoded {
    let mut bytes = vec![0; HEADER_BYTES];
    bytes[..record.fields.len()].copy_from_slice(record.fields);
    // A field of 32 bits cannot have the length of a payload that is longer.
    // A script has no such payload.
    let length = u32::try_from(record.payload.len()).unwrap_or(u32::MAX);
    if !record.kept.position() {
        bytes[POSITION_FIELD..][..8].copy_from_slice(&next_position.to_le_bytes());
    }
    if !record.kept.stored_length() {
        bytes[STORED_LENGTH_FIELD..][..4].copy_from_slice(&length.to_le_bytes());
    }
    if !record.kept.raw_length() {
        bytes[RAW_LENGTH_FIELD..][..4].copy_from_slice(&length.to_le_bytes());
    }
    if !record.kept.payload_checksum() {
        let checksum = crc32(record.payload);
        bytes[PAYLOAD_CHECKSUM_FIELD..][..4].copy_from_slice(&checksum.to_le_bytes());
    }
    let checksum = crc32(&bytes[..HEADER_CHECKSUM_FIELD]);
    bytes[HEADER_CHECKSUM_FIELD..].copy_from_slice(&checksum.to_le_bytes());

    // Only an output record moves the reader.
    let raw_length = if bytes[0] == KIND_OUTPUT {
        u32::from_le_bytes(field(&bytes, RAW_LENGTH_FIELD))
    } else {
        0
    };
    bytes.extend_from_slice(record.payload);
    Encoded {
        bytes,
        next_position: next_position.saturating_add(u64::from(raw_length)),
    }
}

fn field(header: &[u8], offset: usize) -> [u8; 4] {
    header[offset..offset + 4]
        .try_into()
        .expect("the field is in the header")
}

/// CRC-32 with the polynomial of IEEE 802.3, as the journal uses it.
fn crc32(bytes: &[u8]) -> u32 {
    const POLYNOMIAL: u32 = 0xEDB8_8320;
    let mut value = u32::MAX;
    for byte in bytes {
        value ^= u32::from(*byte);
        for _ in 0..8 {
            let carry = value & 1;
            value = (value >> 1) ^ (POLYNOMIAL * carry);
        }
    }
    !value
}
