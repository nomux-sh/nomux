//! Reads one small file of a session state directory.

use std::fs::{self, File};
use std::io::{self, Read};
use std::path::Path;

/// Reads the regular file `path`, which has `limit` bytes at most.
///
/// The file type is examined before the file is opened: to open a FIFO
/// stops the caller until a writer comes.
///
/// # Errors
///
/// Returns an error if the file cannot be read, is not a regular file or is
/// longer than `limit`.
pub(super) fn read(path: &Path, limit: u64) -> io::Result<Vec<u8>> {
    if !fs::symlink_metadata(path)?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "it is not a regular file",
        ));
    }
    let mut bytes = Vec::new();
    // The file can grow after the look at its length.
    File::open(path)?.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("it has more than {limit} bytes"),
        ));
    }
    Ok(bytes)
}
