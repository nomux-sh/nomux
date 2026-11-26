//! The runtime directory: the private directory of the user that contains
//! the socket of the daemon, its lock file and the holder directories.
//! `nomux_core::runtime_directory` has its path and the rule for the paths
//! of the sockets.

use std::fs::{self, DirBuilder};
use std::io;
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
use std::path::{Path, PathBuf};

use nomux_core::runtime_directory::link_socket_directory;

const OWNER_ONLY: u32 = 0o700;
const GROUP_AND_OTHERS: u32 = 0o077;

/// Prepares the runtime directory of the user and gives the path through
/// which the daemon opens it: `~/.nomux/run`, or its short link if that
/// path is too long for a socket.
pub(crate) fn open() -> io::Result<PathBuf> {
    let path = nomux_core::runtime_directory::locate()?;
    prepare(&path)?;
    link_socket_directory(&path)
}

/// Makes the directory with mode 0700 if it does not exist. Fails if the
/// directory belongs to a different user or is open to other users: the
/// socket of the daemon is safe only in a private directory.
fn prepare(path: &Path) -> io::Result<()> {
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

#[cfg(test)]
mod tests {
    use nomux_core::runtime_directory::LONGEST_SOCKET_NAME;
    use nomux_holder_protocol::discovery::{SOCKET_FILE, pooled_holder_directory};

    use super::*;
    use crate::session_id;

    #[test]
    fn the_rule_of_the_socket_paths_knows_the_longest_socket_of_the_daemon() {
        let runtime_directory = Path::new("/run");
        let session_id = session_id::random().unwrap();
        let socket = pooled_holder_directory(runtime_directory, &session_id).join(SOCKET_FILE);
        assert_eq!(
            socket.as_os_str().len(),
            runtime_directory.as_os_str().len() + LONGEST_SOCKET_NAME
        );
    }
}
