//! The last bytes that an extension wrote to its standard error.

use std::collections::VecDeque;
use std::sync::{Mutex, PoisonError};

/// A log of fixed size. When it is full, the oldest bytes go away. Thus an
/// extension cannot make the memory of the daemon grow.
#[derive(Debug)]
pub(crate) struct StderrLog {
    capacity: usize,
    bytes: Mutex<VecDeque<u8>>,
}

impl StderrLog {
    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            capacity,
            bytes: Mutex::new(VecDeque::new()),
        }
    }

    pub(crate) fn append(&self, chunk: &[u8]) {
        let newest = &chunk[chunk.len().saturating_sub(self.capacity)..];
        let mut bytes = self.bytes.lock().unwrap_or_else(PoisonError::into_inner);
        let overflow = (bytes.len() + newest.len()).saturating_sub(self.capacity);
        bytes.drain(..overflow);
        bytes.extend(newest);
    }

    pub(crate) fn contents(&self) -> Vec<u8> {
        let bytes = self.bytes.lock().unwrap_or_else(PoisonError::into_inner);
        bytes.iter().copied().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_log_keeps_only_the_newest_bytes() {
        let log = StderrLog::new(4);

        log.append(b"ab");
        log.append(b"cde");
        assert_eq!(log.contents(), b"bcde");

        log.append(b"0123456789");
        assert_eq!(log.contents(), b"6789");
    }
}
