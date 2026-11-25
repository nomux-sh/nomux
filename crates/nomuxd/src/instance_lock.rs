//! The rule that one daemon at a time uses a runtime directory.
//!
//! The daemon holds an exclusive lock on a file in the runtime directory for
//! all of its life. The operating system releases the lock when the process
//! ends, also after `kill -9`. A second daemon cannot take the lock: it ends
//! with an error before it removes the socket of the first daemon or takes a
//! lease.
//!
//! A daemon that replaces the first daemon (`--replace`) tells it to stop
//! and waits for the lock. The first daemon answers its requests, removes
//! its socket and releases the lock, thus the two daemons never serve at
//! the same time.

use std::fs::{File, OpenOptions, TryLockError};
use std::io::{self, Read, Seek, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::process;
use std::thread::sleep;
use std::time::{Duration, Instant};

use rustix::process::{Pid, Signal, kill_process};

use crate::arguments::RunningDaemon;

/// The name of the lock file. The name has a `.`, thus it is not a session
/// identifier.
const LOCK_FILE: &str = "nomuxd.lock";
const OWNER_READ_WRITE: u32 = 0o600;
/// The longest time that a daemon waits for the daemon that it replaces. A
/// daemon that stops waits for its requests, and a request has 10 seconds
/// at most.
const REPLACE_TIMEOUT: Duration = Duration::from_secs(15);
const LOCK_INTERVAL: Duration = Duration::from_millis(20);

/// The lock of one runtime directory. The lock is released when this value
/// is dropped.
#[derive(Debug)]
pub(crate) struct InstanceLock {
    _file: File,
}

impl InstanceLock {
    /// Takes the lock and writes the process ID of this daemon into the
    /// lock file. If a different daemon has the lock, fails or replaces
    /// that daemon, as `running_daemon` says.
    pub(crate) fn acquire(
        runtime_directory: &Path,
        running_daemon: RunningDaemon,
    ) -> io::Result<Self> {
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(OWNER_READ_WRITE)
            .open(runtime_directory.join(LOCK_FILE))?;
        match file.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) if running_daemon == RunningDaemon::Replace => {
                take_over(&mut file)?;
            }
            Err(TryLockError::WouldBlock) => {
                return Err(already_running(&mut file, runtime_directory));
            }
            Err(TryLockError::Error(error)) => return Err(error),
        }
        file.set_len(0)?;
        // A take-over read the file. The process ID goes to the start.
        file.rewind()?;
        writeln!(file, "{}", process::id())?;
        Ok(Self { _file: file })
    }
}

/// Sends `SIGTERM` to the daemon that has the lock and waits until this
/// process has the lock.
fn take_over(lock_file: &mut File) -> io::Result<()> {
    let owner = stable_owner(lock_file)?;
    kill_process(owner, Signal::TERM)?;
    let deadline = Instant::now() + REPLACE_TIMEOUT;
    loop {
        match lock_file.try_lock() {
            Ok(()) => return Ok(()),
            Err(TryLockError::WouldBlock) if Instant::now() < deadline => sleep(LOCK_INTERVAL),
            Err(TryLockError::WouldBlock) => {
                return Err(io::Error::other(format!(
                    "the daemon with the process ID {} did not stop in {} seconds",
                    owner.as_raw_nonzero(),
                    REPLACE_TIMEOUT.as_secs()
                )));
            }
            Err(TryLockError::Error(error)) => return Err(error),
        }
    }
}

/// The process ID in the lock file. A daemon writes it immediately after it
/// takes the lock. Until then the file has the process ID of an earlier
/// daemon, which can be a different program now. Thus the value counts only
/// if two reads give the same one.
fn stable_owner(lock_file: &mut File) -> io::Result<Pid> {
    let first = read_owner(lock_file)?;
    sleep(LOCK_INTERVAL);
    let second = read_owner(lock_file)?;
    first
        .filter(|_| first == second)
        .and_then(Pid::from_raw)
        .ok_or_else(|| io::Error::other("the lock file does not name the daemon that runs"))
}

fn read_owner(lock_file: &mut File) -> io::Result<Option<i32>> {
    let mut owner = String::new();
    lock_file.rewind()?;
    lock_file.read_to_string(&mut owner)?;
    Ok(owner.trim().parse().ok())
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
