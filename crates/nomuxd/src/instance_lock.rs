//! The rule that one daemon at a time uses a runtime directory.
//!
//! The daemon holds an exclusive lock on a file in the runtime directory for
//! all of its life. The operating system releases the lock when the process
//! ends, also after `kill -9`. A second daemon cannot take the lock: it ends
//! with an error before it removes the socket of the first daemon or takes a
//! lease.

use std::fs::{File, OpenOptions, TryLockError};
use std::io::{self, Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::process;

/// The name of the lock file. The name has a `.`, thus it is not a session
/// identifier.
const LOCK_FILE: &str = "nomuxd.lock";
const OWNER_READ_WRITE: u32 = 0o600;

/// The lock of one runtime directory. The lock is released when this value
/// is dropped.
#[derive(Debug)]
pub(crate) struct InstanceLock {
    _file: File,
}

impl InstanceLock {
    /// Takes the lock and writes the process ID of this daemon into the
    /// lock file. Fails if a different daemon has the lock.
    pub(crate) fn acquire(runtime_directory: &Path) -> io::Result<Self> {
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(OWNER_READ_WRITE)
            .open(runtime_directory.join(LOCK_FILE))?;
        match file.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => {
                return Err(already_running(&mut file, runtime_directory));
            }
            Err(TryLockError::Error(error)) => return Err(error),
        }
        file.set_len(0)?;
        writeln!(file, "{}", process::id())?;
        Ok(Self { _file: file })
    }
}

fn already_running(lock_file: &mut File, runtime_directory: &Path) -> io::Error {
    let mut owner = String::new();
    // The process ID is only a help for the person who reads the message.
    let owner = match lock_file.read_to_string(&mut owner) {
        Ok(_) if !owner.trim().is_empty() => format!(" (process ID {})", owner.trim()),
        _ => String::new(),
    };
    io::Error::other(format!(
        "a daemon already runs for {}{owner}",
        runtime_directory.display()
    ))
}
