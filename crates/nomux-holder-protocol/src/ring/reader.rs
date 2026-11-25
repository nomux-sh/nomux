//! The reading side of the ring buffer: the daemon.

use std::error::Error;
use std::fmt;
use std::io;
use std::os::fd::AsFd;
use std::sync::atomic::{Ordering, fence};

use super::layout::{
    CAPACITY_OFFSET, CLAIMED_OFFSET, COMMITTED_OFFSET, DATA_OFFSET_OFFSET, HEADER_LENGTH,
    LAYOUT_VERSION, LAYOUT_VERSION_OFFSET, MAGIC, MAGIC_OFFSET, is_valid_capacity,
};
use super::mapping::Mapping;

/// The writer wrote over bytes that the reader did not read yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Overrun {
    /// The number of bytes that the reader did not get. The reader continues
    /// after them.
    pub lost: u64,
}

impl fmt::Display for Overrun {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "the ring buffer reader lost {} bytes", self.lost)
    }
}

impl Error for Overrun {}

/// Reads terminal output from the ring buffer of a holder.
#[derive(Debug)]
pub struct RingReader {
    mapping: Mapping,
    data_offset: usize,
    capacity: usize,
    position: u64,
}

impl RingReader {
    /// Maps the descriptor that came with the hello. The reader starts at the
    /// oldest byte that the ring buffer still has.
    ///
    /// # Errors
    ///
    /// Returns an error if `fstat` or `mmap` fails, or if the region does not
    /// have a valid header of layout version 1.
    pub fn map(memory: impl AsFd) -> io::Result<Self> {
        let memory = memory.as_fd();
        let length = usize::try_from(rustix::fs::fstat(memory)?.st_size)
            .ok()
            .filter(|length| *length >= HEADER_LENGTH)
            .ok_or_else(|| invalid("the region is smaller than the header"))?;
        let mapping = Mapping::read_only(memory, length)?;

        let mut header = [0u8; HEADER_LENGTH];
        mapping.copy_out(0, &mut header);
        if header[MAGIC_OFFSET..MAGIC_OFFSET + MAGIC.len()] != MAGIC {
            return Err(invalid("the region is not a ring buffer"));
        }
        if read_u32(&header, LAYOUT_VERSION_OFFSET) != LAYOUT_VERSION {
            return Err(invalid("the layout version is not supported"));
        }
        let data_offset = read_u32(&header, DATA_OFFSET_OFFSET) as usize;
        let capacity = usize::try_from(read_u64(&header, CAPACITY_OFFSET))
            .ok()
            .filter(|capacity| is_valid_capacity(*capacity))
            .ok_or_else(|| invalid("the capacity is not valid"))?;
        let fits = data_offset >= HEADER_LENGTH
            && data_offset
                .checked_add(capacity)
                .is_some_and(|end| end <= length);
        if !fits {
            return Err(invalid("the data is outside the region"));
        }

        let oldest = mapping
            .load_u64(CLAIMED_OFFSET)
            .saturating_sub(capacity as u64);
        Ok(Self {
            mapping,
            data_offset,
            capacity,
            position: oldest,
        })
    }

    /// The position of the next byte that [`read`](Self::read) returns.
    #[must_use]
    pub fn position(&self) -> u64 {
        self.position
    }

    /// The committed position of the writer.
    #[must_use]
    pub fn write_position(&self) -> u64 {
        self.mapping.load_u64(COMMITTED_OFFSET)
    }

    /// Copies the next bytes into `destination` and returns their number.
    /// Returns 0 if the reader has all the bytes that the writer committed.
    ///
    /// # Errors
    ///
    /// Returns [`Overrun`] if the writer wrote over the bytes. The content of
    /// `destination` is then not valid, and the reader continues at the
    /// write position.
    pub fn read(&mut self, destination: &mut [u8]) -> Result<usize, Overrun> {
        let committed = self.write_position();
        let available = committed.wrapping_sub(self.position);
        if available > self.capacity as u64 {
            return Err(self.skip_to(committed));
        }
        let count = usize::try_from(available).map_or(destination.len(), |available| {
            available.min(destination.len())
        });
        let index = self.data_index();
        let first = count.min(self.capacity - index);
        self.mapping
            .copy_out(self.data_offset + index, &mut destination[..first]);
        self.mapping
            .copy_out(self.data_offset, &mut destination[first..count]);

        // The copy must be complete before the claim is loaded: see the
        // fence in the writer.
        fence(Ordering::SeqCst);
        let claimed = self.mapping.load_u64(CLAIMED_OFFSET);
        if claimed.wrapping_sub(self.position) > self.capacity as u64 {
            return Err(self.skip_to(self.write_position()));
        }
        self.position += count as u64;
        Ok(count)
    }

    fn skip_to(&mut self, committed: u64) -> Overrun {
        let lost = committed.wrapping_sub(self.position);
        self.position = committed;
        Overrun { lost }
    }

    fn data_index(&self) -> usize {
        let mask = self.capacity as u64 - 1;
        usize::try_from(self.position & mask).expect("the index is smaller than the capacity")
    }
}

fn read_u32(header: &[u8; HEADER_LENGTH], offset: usize) -> u32 {
    let mut bytes = [0u8; 4];
    bytes.copy_from_slice(&header[offset..offset + 4]);
    u32::from_le_bytes(bytes)
}

fn read_u64(header: &[u8; HEADER_LENGTH], offset: usize) -> u64 {
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&header[offset..offset + 8]);
    u64::from_le_bytes(bytes)
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
