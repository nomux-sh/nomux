//! Makes the end of a child process visible to `poll`.
//!
//! The operating system reports the end of a child with the `SIGCHLD`
//! signal. The signal handler writes one byte to a pipe, and the event loop
//! polls the other end. Thus the holder needs no timer and no thread to see
//! that the child ended.

use std::io::{self, PipeReader, Read};
use std::os::fd::{AsFd, BorrowedFd};

use rustix::fs::{OFlags, fcntl_getfl, fcntl_setfl};
use signal_hook::consts::SIGCHLD;

pub(crate) struct ChildExitSignal {
    reader: PipeReader,
}

impl ChildExitSignal {
    /// Installs the signal handler. Call it before the child starts.
    pub(crate) fn install() -> io::Result<Self> {
        let (reader, writer) = io::pipe()?;
        let flags = fcntl_getfl(&reader)?;
        fcntl_setfl(&reader, flags | OFlags::NONBLOCK)?;
        signal_hook::low_level::pipe::register(SIGCHLD, writer)?;
        Ok(Self { reader })
    }

    /// Readable after a child ended.
    pub(crate) fn descriptor(&self) -> BorrowedFd<'_> {
        self.reader.as_fd()
    }

    /// Removes the bytes of the signals that arrived.
    pub(crate) fn clear(&mut self) -> io::Result<()> {
        let mut bytes = [0u8; 64];
        loop {
            match self.reader.read(&mut bytes) {
                Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(()),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Err(error),
            }
        }
    }
}
