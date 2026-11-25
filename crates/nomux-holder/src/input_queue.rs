//! The input bytes that the terminal did not accept yet.

use std::collections::VecDeque;
use std::os::fd::AsFd;

use rustix::io::Errno;

/// The event loop stops reading requests while the queue has this number of
/// bytes or more. Thus a child that does not read its input makes the daemon
/// wait, and the queue does not grow without a limit.
const BACKLOG_LIMIT: usize = 64 * 1024;

#[derive(Default)]
pub(crate) struct InputQueue {
    pending: VecDeque<u8>,
}

impl InputQueue {
    pub(crate) fn push(&mut self, bytes: &[u8]) {
        self.pending.extend(bytes);
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    pub(crate) fn is_backlogged(&self) -> bool {
        self.pending.len() >= BACKLOG_LIMIT
    }

    pub(crate) fn clear(&mut self) {
        self.pending.clear();
    }

    /// Writes to the non-blocking `terminal` until the queue is empty or the
    /// terminal accepts no more bytes.
    pub(crate) fn flush(&mut self, terminal: impl AsFd) -> Result<(), Errno> {
        loop {
            let (front, _) = self.pending.as_slices();
            if front.is_empty() {
                return Ok(());
            }
            match rustix::io::write(&terminal, front) {
                Ok(written) => {
                    self.pending.drain(..written);
                }
                Err(Errno::AGAIN) => return Ok(()),
                Err(Errno::INTR) => {}
                Err(error) => return Err(error),
            }
        }
    }
}
