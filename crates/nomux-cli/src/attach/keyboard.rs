//! The keys that the user types during an attach.

use std::io::{self, Read};
use std::thread;

use tokio::sync::mpsc;

/// The reads of the terminal that wait for the connection. A paste comes
/// as many reads.
const READ_QUEUE: usize = 16;
const READ_BYTES: usize = 4096;

/// Starts a thread that reads the standard input, and returns the bytes of
/// each read. The channel closes when the terminal closes.
///
/// A read of a terminal cannot be stopped, thus it has its own thread. The
/// thread ends with the process.
pub(super) fn read_keys() -> mpsc::Receiver<Vec<u8>> {
    let (keys, reads) = mpsc::channel(READ_QUEUE);
    thread::spawn(move || {
        let mut terminal = io::stdin().lock();
        let mut buffer = [0u8; READ_BYTES];
        loop {
            let count = match terminal.read(&mut buffer) {
                Ok(0) => return,
                Ok(count) => count,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(_) => return,
            };
            if keys.blocking_send(buffer[..count].to_vec()).is_err() {
                return;
            }
        }
    });
    reads
}
