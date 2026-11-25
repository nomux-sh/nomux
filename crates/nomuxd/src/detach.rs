//! Starts the daemon as a process that continues after its starter ends.

use std::env;
use std::fs::OpenOptions;
use std::io;
use std::os::unix::fs::OpenOptionsExt;
use std::process::{Command, Stdio};

use crate::runtime_directory;

/// The log of a detached daemon, in the runtime directory. The name has a
/// `.`, thus it is not a session identifier.
const LOG_FILE: &str = "nomuxd.log";
const OWNER_READ_WRITE: u32 = 0o600;

/// Starts a second `nomuxd` process that runs the daemon, and returns.
///
/// This process ends immediately after that, thus the daemon has no parent
/// that waits for it. The daemon leaves the session of its starter itself.
/// Its standard error goes to the log file, because a detached process has
/// no terminal.
pub(crate) fn start_detached_daemon() -> io::Result<()> {
    let runtime_directory = runtime_directory::locate()?;
    runtime_directory::prepare(&runtime_directory)?;
    let log = OpenOptions::new()
        .append(true)
        .create(true)
        .mode(OWNER_READ_WRITE)
        .open(runtime_directory.join(LOG_FILE))?;
    Command::new(env::current_exe()?)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(log)
        .spawn()?;
    Ok(())
}
