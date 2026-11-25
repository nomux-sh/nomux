//! Finds the holders of a runtime directory and tells live ones from stale
//! ones.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::process_identity::ProcessIdentity;
use super::{PID_FILE, READY_FILE, SOCKET_FILE};
use crate::SessionId;

/// Why a holder directory has no live holder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StaleReason {
    /// The `pid` file does not exist or cannot be parsed.
    InvalidPidFile,
    /// No process has the recorded process ID.
    ProcessGone,
    /// A process has the recorded process ID, but it has a different start
    /// time: the operating system used the process ID again.
    ProcessReplaced,
}

/// The state of one holder directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HolderState {
    /// The holder runs and its `ready` file exists. The daemon can connect.
    Live,
    /// The holder runs, but it did not write `ready` yet.
    Starting,
    /// The holder does not run. The directory can be removed.
    Stale(StaleReason),
}

/// One holder directory that [`scan`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredHolder {
    /// The session of the holder.
    pub session_id: SessionId,
    /// The holder directory.
    pub directory: PathBuf,
    /// The state of the holder at the time of the scan.
    pub state: HolderState,
}

impl DiscoveredHolder {
    /// The path of the control socket.
    #[must_use]
    pub fn socket_path(&self) -> PathBuf {
        self.directory.join(SOCKET_FILE)
    }

    /// Removes the holder directory and its files. Call it only for a stale
    /// holder.
    ///
    /// # Errors
    ///
    /// Returns the error of the file system.
    pub fn remove_directory(&self) -> io::Result<()> {
        fs::remove_dir_all(&self.directory)
    }
}

/// Reads the holder directories of `runtime_directory`.
///
/// An entry that is not a directory, or whose name is not a session
/// identifier, is not a holder directory and is ignored. A runtime directory
/// that does not exist has no holders.
///
/// A holder that starts makes its directory and then writes `pid`. A scan
/// between these two steps reports `Stale(InvalidPidFile)` for it.
///
/// # Errors
///
/// Returns an error if the runtime directory or a `pid` file cannot be
/// read, or if the operating system does not give the start time of a
/// process.
pub fn scan(runtime_directory: &Path) -> io::Result<Vec<DiscoveredHolder>> {
    let entries = match fs::read_dir(runtime_directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut holders = Vec::new();
    for entry in entries {
        let entry = entry?;
        let session_id = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse().ok());
        let Some(session_id) = session_id else {
            continue;
        };
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let directory = entry.path();
        let state = holder_state(&directory)?;
        holders.push(DiscoveredHolder {
            session_id,
            directory,
            state,
        });
    }
    Ok(holders)
}

fn holder_state(directory: &Path) -> io::Result<HolderState> {
    let Some(recorded) = read_pid_file(&directory.join(PID_FILE))? else {
        return Ok(HolderState::Stale(StaleReason::InvalidPidFile));
    };
    let state = match ProcessIdentity::of(recorded.pid)? {
        None => HolderState::Stale(StaleReason::ProcessGone),
        Some(running) if running != recorded => HolderState::Stale(StaleReason::ProcessReplaced),
        Some(_) if directory.join(READY_FILE).exists() => HolderState::Live,
        Some(_) => HolderState::Starting,
    };
    Ok(state)
}

fn read_pid_file(path: &Path) -> io::Result<Option<ProcessIdentity>> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(text.parse().ok()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}
