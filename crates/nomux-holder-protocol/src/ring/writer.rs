//! The writing side of the ring buffer: the holder.

use std::io;
use std::os::fd::AsFd;
use std::sync::atomic::{Ordering, fence};

use super::layout::{
    CAPACITY_OFFSET, CLAIMED_OFFSET, COMMITTED_OFFSET, DATA_OFFSET, DATA_OFFSET_OFFSET,
    HEADER_LENGTH, LAYOUT_VERSION, LAYOUT_VERSION_OFFSET, MAGIC, MAGIC_OFFSET, is_valid_capacity,
    region_length,
};
use crate::mapping::Mapping;

/// Writes terminal output into the ring buffer.
///
/// The writer keeps its position in private memory and never reads the
/// shared region, thus a process that damages the region cannot stop the
/// writer.
#[derive(Debug)]
pub struct RingWriter {
    mapping: Mapping,
    capacity: usize,
    position: u64,
}

impl RingWriter {
    /// Maps `memory` for writing and writes the header.
    ///
    /// `memory` must be shared memory that is new, filled with zero and at
    /// least [`region_length`]`(capacity)` bytes long.
    ///
    /// # Errors
    ///
    /// Returns an error if `capacity` is not a power of two that is at least
    /// [`MIN_CAPACITY`](super::MIN_CAPACITY), or if `mmap` fails.
    pub fn create(memory: impl AsFd, capacity: usize) -> io::Result<Self> {
        if !is_valid_capacity(capacity) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "the ring buffer capacity must be a power of two of 4096 or more",
            ));
        }
        let mut mapping = Mapping::writable(memory.as_fd(), region_length(capacity))?;
        let header = mapping.slice_mut(0, HEADER_LENGTH);
        header[MAGIC_OFFSET..MAGIC_OFFSET + MAGIC.len()].copy_from_slice(&MAGIC);
        header[LAYOUT_VERSION_OFFSET..LAYOUT_VERSION_OFFSET + 4]
            .copy_from_slice(&LAYOUT_VERSION.to_le_bytes());
        header[DATA_OFFSET_OFFSET..DATA_OFFSET_OFFSET + 4]
            .copy_from_slice(&DATA_OFFSET.to_le_bytes());
        header[CAPACITY_OFFSET..CAPACITY_OFFSET + 8]
            .copy_from_slice(&(capacity as u64).to_le_bytes());
        Ok(Self {
            mapping,
            capacity,
            position: 0,
        })
    }

    /// The committed position: the number of bytes written since the start.
    #[must_use]
    pub fn position(&self) -> u64 {
        self.position
    }

    /// Writes one chunk. `fill` gets a part of the ring buffer of `limit`
    /// bytes at most, writes bytes at its start and returns their number.
    /// `fill` must not write more bytes than it returns.
    ///
    /// The chunk ends at the end of the data region, thus it can be shorter
    /// than `limit`. During the call, the claim takes the length of the
    /// chunk from the oldest data: a reader that is there is overrun. A small
    /// `limit` keeps more history readable.
    ///
    /// # Errors
    ///
    /// Returns the error of `fill`. The committed position does not change
    /// then.
    pub fn write_with<E>(
        &mut self,
        limit: usize,
        fill: impl FnOnce(&mut [u8]) -> Result<usize, E>,
    ) -> Result<usize, E> {
        let index = self.data_index();
        let length = limit.min(self.capacity - index);
        self.mapping
            .store_u64(CLAIMED_OFFSET, self.position + length as u64);
        // A reader must see the claim before it can see a byte of the chunk.
        // The release store does not order the writes that come after it.
        fence(Ordering::SeqCst);
        let chunk = self.mapping.slice_mut(HEADER_LENGTH + index, length);
        let written = fill(chunk).map(|written| written.min(length));
        if let Ok(written) = written {
            self.position += written as u64;
            self.mapping.store_u64(COMMITTED_OFFSET, self.position);
        }
        // The bytes after the committed position were not changed.
        self.mapping.store_u64(CLAIMED_OFFSET, self.position);
        written
    }

    fn data_index(&self) -> usize {
        let mask = self.capacity as u64 - 1;
        usize::try_from(self.position & mask).expect("the index is smaller than the capacity")
    }
}
