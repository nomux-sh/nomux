//! The shared memory that carries the redraw sequence of a screen from a
//! holder to the daemon.
//!
//! A redraw sequence can be longer than the payload of a frame, and the
//! holder does not keep a reply that the socket does not take. Thus the
//! holder writes the sequence into new shared memory and sends a read-only
//! descriptor of that memory together with the
//! [`Reply::Redraw`](crate::wire::Reply::Redraw) message.
//!
//! The memory has no header. The sequence starts at offset 0 and the reply
//! has its length. The holder does not change the memory after it sends the
//! descriptor.

use std::io;
use std::os::fd::AsFd;

use crate::mapping::Mapping;

/// The largest number of bytes of a redraw sequence. If the sequence of a
/// screen is longer, the holder answers with
/// [`ErrorCode::RedrawTooLarge`](crate::wire::ErrorCode::RedrawTooLarge).
pub const MAX_LENGTH: usize = 8 * 1024 * 1024;

/// Writes `sequence` at the start of `memory`. The holder calls this.
///
/// `memory` must be shared memory that is at least `sequence.len()` bytes
/// long.
///
/// # Errors
///
/// Returns the error of `mmap`. An empty sequence is an error.
pub fn write(memory: impl AsFd, sequence: &[u8]) -> io::Result<()> {
    let mut mapping = Mapping::writable(memory.as_fd(), sequence.len())?;
    mapping
        .slice_mut(0, sequence.len())
        .copy_from_slice(sequence);
    Ok(())
}

/// Copies the redraw sequence of `length` bytes out of the memory that came
/// with a [`Reply::Redraw`](crate::wire::Reply::Redraw). The daemon calls
/// this.
///
/// # Errors
///
/// Returns an error if `length` is larger than [`MAX_LENGTH`] or than the
/// memory, or if `fstat` or `mmap` fails.
pub fn read(memory: impl AsFd, length: u32) -> io::Result<Vec<u8>> {
    let memory = memory.as_fd();
    let length = usize::try_from(length)
        .ok()
        .filter(|length| *length <= MAX_LENGTH)
        .ok_or_else(|| invalid("the redraw sequence is longer than the limit"))?;
    let fits = usize::try_from(rustix::fs::fstat(memory)?.st_size)
        .is_ok_and(|memory_length| length <= memory_length);
    if !fits {
        return Err(invalid("the redraw sequence is longer than the memory"));
    }
    let mapping = Mapping::read_only(memory, length)?;
    let mut sequence = vec![0; length];
    mapping.copy_out(0, &mut sequence);
    Ok(sequence)
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
