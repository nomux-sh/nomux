//! The socket of the daemon on the local host.
//!
//! The socket is a Unix stream socket in the runtime directory of the user.
//! Three things keep other users out: the directory has mode 0700, the socket
//! has mode 0600, and the listener closes a connection whose peer is not the
//! owner of the socket.

use std::env;
use std::fs::{self, Permissions};
use std::io;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

use tokio::net::{UnixListener, UnixStream};

use crate::EnvelopeStream;

/// The name of the socket of the daemon in the runtime directory. The name
/// has a `.`, thus it is not a session identifier and not a holder directory.
pub const SOCKET_FILE: &str = "nomuxd.sock";

const OWNER_READ_WRITE: u32 = 0o600;

/// The socket of the daemon of this user: `~/.nomux/run/nomuxd.sock`.
/// Returns `None` if the `HOME` variable is not set.
#[must_use]
pub fn default_socket_path() -> Option<PathBuf> {
    let home = env::var_os("HOME").filter(|home| !home.is_empty())?;
    Some(
        PathBuf::from(home)
            .join(".nomux")
            .join("run")
            .join(SOCKET_FILE),
    )
}

/// Connects to the daemon that listens on `path`.
///
/// # Errors
///
/// Returns the error of `connect`. The kind is `NotFound` or
/// `ConnectionRefused` if no daemon listens there.
pub async fn connect(path: &Path) -> io::Result<EnvelopeStream> {
    UnixStream::connect(path).await.map(EnvelopeStream::new)
}

/// The listening socket of the daemon.
#[derive(Debug)]
pub struct LocalListener {
    listener: UnixListener,
    owner: u32,
}

impl LocalListener {
    /// Makes the socket at `path` with mode 0600. The directory of `path`
    /// must be private to the user, and no file can be at `path`.
    ///
    /// # Errors
    ///
    /// Returns the error of `bind`, `chmod` or `stat`.
    pub fn bind(path: &Path) -> io::Result<Self> {
        let listener = UnixListener::bind(path)?;
        fs::set_permissions(path, Permissions::from_mode(OWNER_READ_WRITE))?;
        let owner = fs::metadata(path)?.uid();
        Ok(Self { listener, owner })
    }

    /// Waits for the next connection.
    ///
    /// # Errors
    ///
    /// Returns the error of `accept`. Returns an error of the kind
    /// `PermissionDenied` if the peer is a different user: that connection
    /// is closed. The listener can accept again after each error.
    pub async fn accept(&self) -> io::Result<EnvelopeStream> {
        let (stream, _) = self.listener.accept().await?;
        let peer = stream.peer_cred()?.uid();
        if peer != self.owner {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("the peer is user {peer}, not the owner of the socket"),
            ));
        }
        Ok(EnvelopeStream::new(stream))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_socket_is_private_to_its_owner() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(SOCKET_FILE);
        let _listener = LocalListener::bind(&path).unwrap();

        let mode = fs::metadata(&path).unwrap().mode() & 0o777;
        assert_eq!(mode, OWNER_READ_WRITE);
    }
}
