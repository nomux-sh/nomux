//! The pin of one session on disk: the empty file `pinned` in its state
//! directory. A pinned session is excluded from automatic hibernation.

use std::fs::OpenOptions;
use std::io;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use super::small_file;

const FILE: &str = "pinned";
const OWNER_READ_WRITE: u32 = 0o600;

pub(super) fn is_pinned(session_directory: &Path) -> bool {
    session_directory.join(FILE).is_file()
}

pub(super) fn set(session_directory: &Path, pinned: bool) -> io::Result<()> {
    let path = session_directory.join(FILE);
    if !pinned {
        return small_file::remove(&path);
    }
    OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(OWNER_READ_WRITE)
        .open(path)
        .map(drop)
}
