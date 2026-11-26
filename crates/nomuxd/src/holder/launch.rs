//! Starts a `nomux-holder` process for a session: a new session, or a
//! dormant session that starts again.

use std::error::Error;
use std::ffi::OsString;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::process::{ExitStatus, Stdio};
use std::time::Duration;

use nomux_core::companion_binary;
use nomux_holder_protocol::SessionId;
use nomux_holder_protocol::discovery::{READY_FILE, holder_directory};
use nomux_holder_protocol::wire::WindowSize;
use tokio::process::{Child, Command};
use tokio::time::sleep;
use tracing::debug;

const HOLDER_BINARY: &str = "nomux-holder";
/// The longest time from the start of the holder to its `ready` file.
const READY_TIMEOUT: Duration = Duration::from_secs(5);
/// A holder is ready a few milliseconds after its start.
const READY_CHECK_INTERVAL: Duration = Duration::from_millis(5);
const OWNER_READ_WRITE: u32 = 0o600;

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
    pub(crate) working_directory: &'a Path,
    /// The size of the terminal window. `None` is the default size of the
    /// holder.
    pub(crate) size: Option<WindowSize>,
}

/// Starts a holder for `start` and waits until it is ready. The holder
/// keeps its discovery files in `runtime_directory` and the state of the
/// session in `state_directory`.
///
/// The holder is not tied to the daemon. It makes its own session with
/// `setsid`, it has no pipe to the daemon, and the daemon sets no signal for
/// the end of the parent. Thus the holder continues when the daemon ends.
pub(crate) async fn launch(
    runtime_directory: &Path,
    state_directory: &Path,
    start: HolderStart<'_>,
) -> Result<(), LaunchError> {
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
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(holder_error)
        .spawn()
        .map_err(LaunchError::Spawn)?;

    let directory = holder_directory(runtime_directory, session_id);
    tokio::select! {
        () = ready(&directory) => {
            tokio::spawn(collect_exit_status(holder, session_id.clone()));
            Ok(())
        }
        status = holder.wait() => Err(LaunchError::Exited {
            status: status.map_err(LaunchError::Spawn)?,
            message: last_line(&mut standard_error),
        }),
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

fn last_line(standard_error: &mut File) -> String {
    let mut text = String::new();
    let read = standard_error
        .seek(SeekFrom::Start(0))
        .and_then(|_| standard_error.read_to_string(&mut text));
    match read {
        Ok(_) => text.lines().last().unwrap_or_default().to_owned(),
        Err(_) => String::new(),
    }
}

/// Waits for the end of a holder that this daemon started. Without the wait,
/// the ended holder stays in the process table as a zombie.
async fn collect_exit_status(mut holder: Child, session_id: SessionId) {
    match holder.wait().await {
        Ok(status) => debug!(session = %session_id, %status, "the holder ended"),
        Err(error) => debug!(session = %session_id, %error, "cannot wait for the holder"),
    }
}
