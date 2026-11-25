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

use crate::shared_bytes;

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
    shared_bytes::write(memory.as_fd(), sequence)
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
    shared_bytes::read(memory.as_fd(), length, MAX_LENGTH)
}
