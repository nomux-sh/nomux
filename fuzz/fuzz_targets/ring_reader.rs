//! The ring buffer reader of the daemon on a region that a hostile holder
//! wrote.
//!
//! The input makes the header and the length of the region, and then a
//! sequence of operations: the reader reads and seeks, and the holder changes
//! its two positions to arbitrary values. The reader must not panic and must
//! not read outside the region. Each byte that a read returns must be the
//! byte of the region at the position of the reader.
//!
//! The input has three parts:
//!
//! ```text
//! offset  size  part
//!      0    64  the header of the region
//!     64     2  the length of the region, little-endian
//!     66   9*n  the operations: one byte for the kind, 8 bytes for the value
//! ```
//!
//! The target does not make the region shorter after the reader mapped it.

#![no_main]

use std::fs::File;
use std::os::unix::fs::FileExt;

use libfuzzer_sys::fuzz_target;
use nomux_holder_protocol::ring::{HEADER_LENGTH, RingReader};

/// The fields of the header that the target uses. `ring.rs` of
/// `nomux-holder-protocol` has the layout.
const DATA_OFFSET_FIELD: usize = 12;
const CAPACITY_FIELD: usize = 16;
const CLAIMED_FIELD: u64 = 32;
const COMMITTED_FIELD: u64 = 40;

const REGION_LENGTH_BYTES: usize = 2;
const OPERATION_BYTES: usize = 9;

/// A prime number, thus the data has a different byte at each index of a
/// ring buffer whose capacity is a power of two.
const DATA_PERIOD: usize = 251;

fuzz_target!(|input: &[u8]| {
    let Some((header, rest)) = input.split_first_chunk::<HEADER_LENGTH>() else {
        return;
    };
    let Some((region_length, operations)) = rest.split_first_chunk::<REGION_LENGTH_BYTES>() else {
        return;
    };
    let region = region_with(header, usize::from(u16::from_le_bytes(*region_length)));
    let memory = shared_memory(&region);
    let Ok(mut reader) = RingReader::map(&memory) else {
        return;
    };

    let data = RingData::of_valid_header(header, &region);
    let mut destination = vec![0; usize::from(u16::MAX)];
    for [kind, value @ ..] in operations.as_chunks::<OPERATION_BYTES>().0 {
        let value = u64::from_le_bytes(*value);
        match kind % 4 {
            0 => assert_read_is_correct(&mut reader, &data, &mut destination[..low_16_bits(value)]),
            1 => reader.seek(value),
            2 => store(&memory, CLAIMED_FIELD, value),
            _ => store(&memory, COMMITTED_FIELD, value),
        }
    }
});

/// The bytes of a region of `length` bytes: `header` at the start, as much
/// of it as the region has space for, and then data that the target knows.
fn region_with(header: &[u8; HEADER_LENGTH], length: usize) -> Vec<u8> {
    let mut region: Vec<u8> = (0..length).map(data_byte).collect();
    let header_length = length.min(HEADER_LENGTH);
    region[..header_length].copy_from_slice(&header[..header_length]);
    region
}

fn data_byte(offset: usize) -> u8 {
    u8::try_from(offset % DATA_PERIOD).expect("the period is smaller than 256")
}

fn shared_memory(region: &[u8]) -> File {
    let memory = tempfile::tempfile().expect("the host has a directory for temporary files");
    memory
        .write_all_at(region, 0)
        .expect("the host has space for the region");
    memory
}

/// The holder changes one of its positions.
fn store(memory: &File, field: u64, value: u64) {
    memory
        .write_all_at(&value.to_le_bytes(), field)
        .expect("the field is in the region");
}

fn low_16_bits(value: u64) -> usize {
    usize::from(u16::try_from(value & u64::from(u16::MAX)).expect("the value has 16 bits"))
}

/// The data of a region whose header the reader accepted.
struct RingData<'region> {
    data: &'region [u8],
}

impl<'region> RingData<'region> {
    fn of_valid_header(header: &[u8; HEADER_LENGTH], region: &'region [u8]) -> Self {
        let data_offset = u32::from_le_bytes(field(header, DATA_OFFSET_FIELD));
        let capacity = u64::from_le_bytes(field(header, CAPACITY_FIELD));
        let start = usize::try_from(data_offset).expect("the reader accepted the data offset");
        let capacity = usize::try_from(capacity).expect("the reader accepted the capacity");
        assert!(capacity.is_power_of_two(), "capacity {capacity}");
        Self {
            data: &region[start..start + capacity],
        }
    }

    /// The byte at a position of the ring buffer.
    fn byte_at(&self, position: u64) -> u8 {
        let mask = self.data.len() as u64 - 1;
        let index = usize::try_from(position & mask).expect("the index is in the data");
        self.data[index]
    }
}

fn field<const N: usize>(header: &[u8; HEADER_LENGTH], offset: usize) -> [u8; N] {
    header[offset..offset + N]
        .try_into()
        .expect("the field is in the header")
}

fn assert_read_is_correct(reader: &mut RingReader, data: &RingData<'_>, destination: &mut [u8]) {
    let start = reader.position();
    let Ok(count) = reader.read(destination) else {
        return;
    };
    assert!(count <= destination.len());
    // A hostile holder can put the reader at the largest position.
    assert_eq!(reader.position(), start.wrapping_add(count as u64));
    for (offset, byte) in (0..).zip(&destination[..count]) {
        let position = start.wrapping_add(offset);
        assert_eq!(*byte, data.byte_at(position), "position {position}");
    }
}
