//! The header fields of the ring buffer region. `ring.rs` has the table.

pub(super) const MAGIC: [u8; 8] = *b"NMXRING\0";
pub(super) const LAYOUT_VERSION: u32 = 1;

pub(super) const MAGIC_OFFSET: usize = 0;
pub(super) const LAYOUT_VERSION_OFFSET: usize = 8;
pub(super) const DATA_OFFSET_OFFSET: usize = 12;
pub(super) const CAPACITY_OFFSET: usize = 16;
pub(super) const CLAIMED_OFFSET: usize = 32;
pub(super) const COMMITTED_OFFSET: usize = 40;

/// The value of the data offset field in layout version 1.
pub(super) const DATA_OFFSET: u32 = 64;

/// The number of bytes before the data in layout version 1.
pub const HEADER_LENGTH: usize = DATA_OFFSET as usize;

/// The smallest capacity of a ring buffer.
pub const MIN_CAPACITY: usize = 4096;

/// The number of bytes of shared memory that a ring buffer with `capacity`
/// data bytes needs.
#[must_use]
pub const fn region_length(capacity: usize) -> usize {
    HEADER_LENGTH + capacity
}

pub(super) fn is_valid_capacity(capacity: usize) -> bool {
    capacity.is_power_of_two() && capacity >= MIN_CAPACITY
}
