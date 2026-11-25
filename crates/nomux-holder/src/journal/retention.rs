//! Keeps the journal of a session below its size limit: removes the oldest
//! segment files first.

use std::collections::VecDeque;
use std::fs;
use std::io;
use std::path::PathBuf;

pub(super) struct Retention {
    limit: u64,
    /// The segments that get no more record, the oldest first, each with
    /// the number of its bytes.
    closed: VecDeque<(PathBuf, u64)>,
    closed_length: u64,
}

impl Retention {
    pub(super) fn new(limit: u64) -> Self {
        Self {
            limit,
            closed: VecDeque::new(),
            closed_length: 0,
        }
    }

    /// Adds a segment that is newer than each segment before it.
    pub(super) fn add_closed(&mut self, path: PathBuf, length: u64) {
        self.closed.push_back((path, length));
        self.closed_length += length;
    }

    /// Removes the oldest closed segments until the closed segments and the
    /// open segment of `open_length` bytes are not above the limit. The
    /// open segment is never removed.
    pub(super) fn enforce(&mut self, open_length: u64) -> io::Result<()> {
        while self.closed_length + open_length > self.limit {
            let Some((path, length)) = self.closed.pop_front() else {
                break;
            };
            self.closed_length -= length;
            match fs::remove_file(path) {
                Ok(()) => {}
                // A person or the daemon removed the file.
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }
}
