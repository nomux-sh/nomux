//! Starts a `nomux-holder` process for a session: a new session, a dormant
//! session that starts again, or a pooled holder that waits for a session.

use std::error::Error;
use std::ffi::OsString;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::time::Duration;

use nomux_core::companion_binary;
use nomux_holder_protocol::SessionId;
use nomux_holder_protocol::discovery::{READY_FILE, holder_directory, pooled_holder_directory};
use nomux_holder_protocol::session_state::PANIC_FILE;
use nomux_holder_protocol::wire::WindowSize;
use tokio::process::{Child, ChildStdin, Command};
use tokio::time::sleep;
use tracing::{debug, warn};

use crate::holder_pool::ShellStart;

const HOLDER_BINARY: &str = "nomux-holder";
/// The longest time from the start of the holder to its `ready` file.
const READY_TIMEOUT: Duration = Duration::from_secs(5);
/// A holder is ready a few milliseconds after its start.
const READY_CHECK_INTERVAL: Duration = Duration::from_millis(1);
const OWNER_READ_WRITE: u32 = 0o600;
/// The line after a panic message of a program without a backtrace.
const BACKTRACE_NOTE: &str = "note: run with `RUST_BACKTRACE=1`";

/// Why a holder did not start.
#[derive(Debug)]
pub(crate) enum LaunchError {
    /// The holder process cannot start.
    Spawn(io::Error),
    /// The holder ended before it was ready.
    Exited {
        status: ExitStatus,
        /// The last line that the holder wrote to its standard error.
        message: String,
    },
    /// The holder did not write its `ready` file in time.
    NotReady,
}

impl fmt::Display for LaunchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spawn(error) => write!(formatter, "cannot start {HOLDER_BINARY}: {error}"),
            Self::Exited { status, message } if status.signal().is_some() => {
                write!(formatter, "{HOLDER_BINARY} ended with {status}")?;
                if message.is_empty() {
                    return formatter.write_str(", and it wrote nothing to its standard error");
                }
                write!(formatter, ": {message}")
            }
            Self::Exited { status, message } if message.is_empty() => {
                write!(formatter, "{HOLDER_BINARY} ended with {status}")
            }
            Self::Exited { message, .. } => formatter.write_str(message),
            Self::NotReady => write!(
                formatter,
                "the session was not ready in {} seconds",
                READY_TIMEOUT.as_secs()
            ),
        }
    }
}

impl Error for LaunchError {}

/// The session that a holder starts.
pub(crate) struct HolderStart<'a> {
    pub(crate) session_id: &'a SessionId,
    /// The program and its arguments.
    pub(crate) command: &'a [OsString],
    /// The words of a program that starts the command, for example with a
    /// memory limit. Empty if the holder starts the command itself.
    pub(crate) launcher: &'a [OsString],
    pub(crate) working_directory: &'a Path,
    /// The size of the terminal window. `None` is the default size of the
    /// holder.
    pub(crate) size: Option<WindowSize>,
    /// The holder waits in the pool: it has no session until the daemon
    /// assigns `session_id` to it.
    pub(crate) pooled: bool,
    /// The start file of nomux for the shell of a pooled holder.
    pub(crate) shell_start: Option<&'a ShellStart>,
}

impl HolderStart<'_> {
    /// The discovery directory of the holder.
    pub(crate) fn directory(&self, runtime_directory: &Path) -> PathBuf {
        if self.pooled {
            pooled_holder_directory(runtime_directory, self.session_id)
        } else {
            holder_directory(runtime_directory, self.session_id)
        }
    }
}

/// Starts a holder for `start` and waits until it is ready. The holder
/// keeps its discovery files in `runtime_directory` and the state of the
/// session in `state_directory`.
///
/// The holder of a session is not tied to the daemon. It makes its own
/// session with `setsid`, it has no pipe to the daemon, and the daemon sets
/// no signal for the end of the parent. Thus the holder continues when the
/// daemon ends.
///
/// A pooled holder is tied to the daemon: its standard input is a pipe, and
/// the function returns the write end. When the daemon drops it, also at
/// the end of the daemon, a holder that is still pooled ends.
pub(crate) async fn launch(
    runtime_directory: &Path,
    state_directory: &Path,
    start: HolderStart<'_>,
) -> Result<Option<ChildStdin>, LaunchError> {
    let session_id = start.session_id;
    let mut standard_error =
        unnamed_file(runtime_directory, session_id).map_err(LaunchError::Spawn)?;
    let holder_error = standard_error.try_clone().map_err(LaunchError::Spawn)?;
    let mut holder = Command::new(companion_binary(HOLDER_BINARY));
    holder
        .arg("--session")
        .arg(session_id.as_str())
        .arg("--runtime-directory")
        .arg(runtime_directory)
        .arg("--state-directory")
        .arg(state_directory)
        .arg("--working-directory")
        .arg(start.working_directory);
    if start.pooled {
        holder.arg("--pooled");
    }
    if let Some(shell_start) = start.shell_start {
        holder
            .arg("--directory-fifo")
            .arg(&shell_start.directory_fifo);
        for argument in &shell_start.arguments {
            holder.arg("--start-argument").arg(argument);
        }
        for (name, value) in &shell_start.environment {
            match value {
                Some(value) => holder.env(name, value),
                None => holder.env_remove(name),
            };
        }
    }
    for word in start.launcher {
        holder.arg("--launcher").arg(word);
    }
    if let Some(size) = start.size {
        holder
            .arg("--rows")
            .arg(size.rows.to_string())
            .arg("--columns")
            .arg(size.columns.to_string());
    }
    let mut holder = holder
        .arg("--")
        .args(start.command)
        .stdin(if start.pooled {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::null())
        .stderr(holder_error)
        .spawn()
        .map_err(LaunchError::Spawn)?;
    let pool_pipe = holder.stdin.take();

    let directory = start.directory(runtime_directory);
    tokio::select! {
        () = ready(&directory) => {
            tokio::spawn(collect_exit_status(holder, session_id.clone()));
            Ok(pool_pipe)
        }
        status = holder.wait() => {
            let error = LaunchError::Exited {
                status: status.map_err(LaunchError::Spawn)?,
                message: failure_message(&read_all(&mut standard_error)),
            };
            if let LaunchError::Exited { status, .. } = &error
                && status.signal().is_some()
            {
                warn!(session = %session_id, %error, "a holder ended before it was ready");
            }
            Err(error)
        }
        () = sleep(READY_TIMEOUT) => {
            // The holder gets `SIGKILL`, thus it cannot remove its directory.
            let _ = holder.start_kill();
            tokio::spawn(collect_exit_status(holder, session_id.clone()));
            let _ = fs::remove_dir_all(&directory);
            Err(LaunchError::NotReady)
        }
    }
}

/// A file without a name for the standard error of a holder.
///
/// A pipe is not possible: after the daemon ends, a holder that writes to a
/// pipe of the daemon fails. A file works without a reader, and a file
/// without a name leaves nothing in the runtime directory.
fn unnamed_file(runtime_directory: &Path, session_id: &SessionId) -> io::Result<File> {
    // The name has a `.`, thus it is not a session identifier.
    let path = runtime_directory.join(format!("{session_id}.stderr"));
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(OWNER_READ_WRITE)
        .open(&path)?;
    fs::remove_file(&path)?;
    Ok(file)
}

/// The holder writes `ready` last.
async fn ready(holder_directory: &Path) {
    let ready_file = holder_directory.join(READY_FILE);
    while !ready_file.exists() {
        sleep(READY_CHECK_INTERVAL).await;
    }
}

fn read_all(standard_error: &mut File) -> String {
    let mut text = String::new();
    let read = standard_error
        .seek(SeekFrom::Start(0))
        .and_then(|_| standard_error.read_to_string(&mut text));
    match read {
        Ok(_) => text,
        Err(_) => String::new(),
    }
}

/// What the standard error of a holder that ended says: its last line, or
/// the message of a panic with its location.
fn failure_message(standard_error: &str) -> String {
    let lines: Vec<&str> = standard_error
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.is_empty() && !line.starts_with(BACKTRACE_NOTE))
        .collect();
    let start = lines
        .iter()
        .rposition(|line| line.starts_with("thread '") && line.contains(" panicked at "))
        .unwrap_or(lines.len().saturating_sub(1));
    lines[start..].join(" ")
}

/// Waits for the end of a holder that this daemon started. Without the wait,
/// the ended holder stays in the process table as a zombie.
async fn collect_exit_status(mut holder: Child, session_id: SessionId) {
    match holder.wait().await {
        Ok(status) if status.signal().is_some() || status.code() != Some(0) => warn!(
            session = %session_id,
            %status,
            "the holder ended; after a panic, the file {PANIC_FILE} of the session state directory has its message"
        ),
        Ok(status) => debug!(session = %session_id, %status, "the holder ended"),
        Err(error) => debug!(session = %session_id, %error, "cannot wait for the holder"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIGABRT: i32 = 6;

    #[test]
    fn the_message_of_a_holder_panic_has_its_location() {
        let standard_error = "nomux-holder: connection refused: x\n\nthread 'main' (4711) \
            panicked at crates/nomux-holder/src/pty.rs:12:5:\nindex out of bounds\nnote: run \
            with `RUST_BACKTRACE=1` environment variable to display a backtrace\n";

        assert_eq!(
            failure_message(standard_error),
            "thread 'main' (4711) panicked at crates/nomux-holder/src/pty.rs:12:5: index out of \
             bounds"
        );
        assert_eq!(
            failure_message("an earlier line\nnomux-holder: cannot start the command\n"),
            "nomux-holder: cannot start the command"
        );
        assert_eq!(failure_message(""), "");
    }

    #[test]
    fn a_holder_that_ended_by_a_signal_is_named_with_the_signal() {
        let aborted = |message: &str| {
            LaunchError::Exited {
                status: ExitStatus::from_raw(SIGABRT),
                message: message.to_owned(),
            }
            .to_string()
        };

        assert_eq!(
            aborted(""),
            "nomux-holder ended with signal: 6 (SIGABRT), and it wrote nothing to its standard \
             error"
        );
        assert_eq!(
            aborted("thread 'main' panicked at a.rs:1:1: x"),
            "nomux-holder ended with signal: 6 (SIGABRT): thread 'main' panicked at a.rs:1:1: x"
        );
    }
}
