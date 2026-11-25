//! Bytes that go from a holder to the daemon in new shared memory, because
//! they can be longer than the payload of a frame.
//!
//! The memory has no header. The bytes start at offset 0, and the reply
//! that comes with the descriptor of the memory has their length.

use std::io;
use std::os::fd::BorrowedFd;

use crate::mapping::Mapping;

/// Writes `bytes` at the start of `memory`, which must be at least
/// `bytes.len()` bytes long. Empty `bytes` are an error of `mmap`.
pub(crate) fn write(memory: BorrowedFd<'_>, bytes: &[u8]) -> io::Result<()> {
    let mut mapping = Mapping::writable(memory, bytes.len())?;
    mapping.slice_mut(0, bytes.len()).copy_from_slice(bytes);
    Ok(())
}

/// Copies `length` bytes out of `memory`. Fails if `length` is larger than
/// `max_length` or than the memory.
pub(crate) fn read(memory: BorrowedFd<'_>, length: u32, max_length: usize) -> io::Result<Vec<u8>> {
    let length = usize::try_from(length)
        .ok()
        .filter(|length| *length <= max_length)
        .ok_or_else(|| invalid("the length is larger than the limit"))?;
    let fits = usize::try_from(rustix::fs::fstat(memory)?.st_size)
        .is_ok_and(|memory_length| length <= memory_length);
    if !fits {
        return Err(invalid("the length is larger than the shared memory"));
    }
    let mapping = Mapping::read_only(memory, length)?;
    let mut bytes = vec![0; length];
    mapping.copy_out(0, &mut bytes);
    Ok(bytes)
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
