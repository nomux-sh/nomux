//! Reads and writes one small file of the state directory.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

const OWNER_READ_WRITE: u32 = 0o600;

/// Reads the regular file `path`, which has `limit` bytes at most.
///
/// The file type is examined before the file is opened: to open a FIFO
/// stops the caller until a writer comes.
///
/// # Errors
///
/// Returns an error if the file cannot be read, is not a regular file or is
/// longer than `limit`.
pub(crate) fn read(path: &Path, limit: u64) -> io::Result<Vec<u8>> {
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

/// Replaces the file `name` in `directory` with `bytes` in one step: the
/// bytes go to a new file first, and a rename then puts it in the place of
/// the file.
pub(crate) fn replace(directory: &Path, name: &str, bytes: &[u8]) -> io::Result<()> {
    let new_file = directory.join(format!("{name}.new"));
    OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(OWNER_READ_WRITE)
        .open(&new_file)?
        .write_all(bytes)?;
    fs::rename(new_file, directory.join(name))
}

/// Removes the file `path`. A file that does not exist is removed.
pub(crate) fn remove(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}
