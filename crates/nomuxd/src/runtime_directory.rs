//! The runtime directory: the private directory of the user that contains
//! the socket of the daemon, its lock file and the holder directories.

use std::fs::{self, DirBuilder};
use std::io;
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
use std::path::{Path, PathBuf};

use nomux_holder_protocol::discovery::default_runtime_directory;

const OWNER_ONLY: u32 = 0o700;
const GROUP_AND_OTHERS: u32 = 0o077;

/// The runtime directory of the user: `~/.nomux/run`.
pub(crate) fn locate() -> io::Result<PathBuf> {
    default_runtime_directory().ok_or_else(|| {
        io::Error::other("the HOME variable is not set, thus the runtime directory has no path")
    })
}

/// Makes the directory with mode 0700 if it does not exist. Fails if the
/// directory belongs to a different user or is open to other users: the
/// socket of the daemon is safe only in a private directory.
pub(crate) fn prepare(path: &Path) -> io::Result<()> {
    DirBuilder::new()
        .recursive(true)
        .mode(OWNER_ONLY)
        .create(path)?;
    let metadata = fs::metadata(path)?;
    let owner = metadata.uid();
    if owner != rustix::process::getuid().as_raw() {
        return Err(io::Error::other(format!(
            "the runtime directory {} belongs to user {owner}",
            path.display()
        )));
    }
    let mode = metadata.mode() & 0o777;
    if mode & GROUP_AND_OTHERS != 0 {
        return Err(io::Error::other(format!(
            "the runtime directory {} has mode {mode:o}: other users can open it. \
             Set the mode to 700.",
            path.display()
        )));
    }
    Ok(())
}
