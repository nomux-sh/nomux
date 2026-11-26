//! The name of one session on disk: the file `name` in its state directory.
//! The file has one line: the name. A session without a name has no file.
//!
//! The daemon owns this file, as it owns the restore policy file. The name
//! stays with the session when it is dormant, after a restart of the daemon
//! and of the host, and while the ended session is kept.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use super::small_file;
use crate::session_name::SessionName;

const FILE: &str = "name";
/// The name goes to this file first. A rename then replaces the name file
/// in one step.
const NEW_FILE: &str = "name.new";
const OWNER_READ_WRITE: u32 = 0o600;
/// A name and its line break.
const MAX_LENGTH: u64 = 65;

/// The name in `session_directory`. `None` if the session has no name, or
/// if the file cannot be read or has no valid name: the session then has no
/// name, and it is not damaged.
pub(super) fn read(session_directory: &Path) -> Option<SessionName> {
    let bytes = small_file::read(&session_directory.join(FILE), MAX_LENGTH).ok()?;
    std::str::from_utf8(&bytes).ok()?.trim_end().parse().ok()
}

/// Writes `name` to `session_directory`, or removes the file for `None`.
pub(super) fn write(session_directory: &Path, name: Option<&SessionName>) -> io::Result<()> {
    let Some(name) = name else {
        return match fs::remove_file(session_directory.join(FILE)) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        };
    };
    let new_file = session_directory.join(NEW_FILE);
    OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(OWNER_READ_WRITE)
        .open(&new_file)?
        .write_all(format!("{name}\n").as_bytes())?;
    fs::rename(new_file, session_directory.join(FILE))
}
