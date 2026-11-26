//! Writes the discovery files of this holder. The layout is in
//! `nomux_holder_protocol::discovery`. A pooled holder has the directory of
//! a pooled holder until it gets its session.

use std::fs::{self, DirBuilder};
use std::io;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};

use nomux_holder_protocol::SessionId;
use nomux_holder_protocol::discovery::{
    PID_FILE, ProcessIdentity, READY_FILE, SOCKET_FILE, holder_directory, pooled_holder_directory,
};

const OWNER_ONLY: u32 = 0o700;

/// The discovery directory of this holder. The directory is removed when
/// this value is dropped.
pub(crate) struct DiscoveryDirectory {
    path: PathBuf,
}

impl DiscoveryDirectory {
    /// Makes the directory and writes the `pid` file: the directory of the
    /// session, or the directory of a pooled holder if `pooled`. Fails if
    /// the directory exists.
    pub(crate) fn create(
        runtime_directory: &Path,
        session_id: &SessionId,
        pooled: bool,
    ) -> io::Result<Self> {
        DirBuilder::new()
            .recursive(true)
            .mode(OWNER_ONLY)
            .create(runtime_directory)?;
        let path = if pooled {
            pooled_holder_directory(runtime_directory, session_id)
        } else {
            holder_directory(runtime_directory, session_id)
        };
        DirBuilder::new().mode(OWNER_ONLY).create(&path)?;
        let directory = Self { path };
        let identity = ProcessIdentity::current()?;
        fs::write(directory.path.join(PID_FILE), format!("{identity}\n"))?;
        Ok(directory)
    }

    pub(crate) fn socket_path(&self) -> PathBuf {
        self.path.join(SOCKET_FILE)
    }

    /// Tells the daemon that it can connect. Call it last.
    pub(crate) fn mark_ready(&self) -> io::Result<()> {
        fs::write(self.path.join(READY_FILE), "")
    }

    /// Gives the directory of a pooled holder the name of its session. The
    /// socket keeps working: a connection finds it by the new path.
    pub(crate) fn assign(
        &mut self,
        runtime_directory: &Path,
        session_id: &SessionId,
    ) -> io::Result<()> {
        let session_path = holder_directory(runtime_directory, session_id);
        fs::rename(&self.path, &session_path)?;
        self.path = session_path;
        Ok(())
    }
}

impl Drop for DiscoveryDirectory {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.path) {
            eprintln!(
                "nomux-holder: cannot remove {}: {error}",
                self.path.display()
            );
        }
    }
}
