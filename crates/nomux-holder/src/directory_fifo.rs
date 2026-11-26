//! The named pipe through which a pooled holder gives its command the
//! working directory of the session. The daemon makes it and gives the
//! path, and the start file of the shell reads one record from it before
//! the first prompt.
//!
//! The holder keeps the pipe open for reading and writing from its start.
//! Thus the shell can open the path before or after the assignment, and its
//! open does not wait for a writer. At the assignment, the holder writes the
//! directory and a zero byte. When the shell has read them, the holder
//! closes the pipe and removes the path. The holder removes it also when
//! it ends: a shell that opens the path later gets nothing and stays in
//! its directory.

use std::io;
use std::os::fd::OwnedFd;
use std::path::{Path, PathBuf};

use rustix::fs::{FileType, Mode, OFlags};

pub(crate) struct DirectoryFifo {
    path: PathBuf,
    pipe: OwnedFd,
    /// The holder wrote the directory.
    sent: bool,
}

impl DirectoryFifo {
    /// Opens the named pipe at `path`. Fails if the file at `path` is not a
    /// named pipe.
    pub(crate) fn open(path: &Path) -> io::Result<Self> {
        let pipe = rustix::fs::open(
            path,
            OFlags::RDWR | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        )?;
        if FileType::from_raw_mode(rustix::fs::fstat(&pipe)?.st_mode) != FileType::Fifo {
            return Err(io::Error::other(format!(
                "{} is not a named pipe",
                path.display()
            )));
        }
        Ok(Self {
            path: path.to_owned(),
            pipe,
            sent: false,
        })
    }

    /// Writes `directory` and a zero byte. A path has no zero byte, thus the
    /// shell reads the path as it is. The pipe is empty and a path is
    /// shorter than its capacity, thus the write is complete.
    pub(crate) fn send(&mut self, directory: &[u8]) -> io::Result<()> {
        let record = [directory, &[0]].concat();
        let written = rustix::io::write(&self.pipe, &record)?;
        if written != record.len() {
            return Err(io::Error::other(
                "the named pipe took a part of the directory",
            ));
        }
        self.sent = true;
        Ok(())
    }

    /// `true` after the shell has read the directory: the pipe is empty.
    pub(crate) fn is_read(&self) -> bool {
        self.sent && rustix::io::ioctl_fionread(&self.pipe).is_ok_and(|unread| unread == 0)
    }
}

impl Drop for DirectoryFifo {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
