//! The last hibernation of one session on disk: the file `hibernation` in
//! its state directory, as JSON. The daemon owns it, as it owns the restore
//! policy. The file is written before the daemon stops the command, thus a
//! daemon that starts after the stop knows that the session is hibernated
//! and not ended, and it knows the resume command.

use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::small_file;
use crate::session_memory::MemoryMeasure;

const FILE: &str = "hibernation";
/// A resume command has 512 bytes at most, and an error a few hundred.
const MAX_LENGTH: u64 = 16 * 1024;

/// Why the daemon hibernated a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum HibernationReason {
    Count,
    SessionMemory,
    TotalMemory,
    MemoryPressure,
    Request,
}

/// The last hibernation of a session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct HibernationRecord {
    /// Seconds after the Unix epoch.
    pub(crate) hibernated_at: u64,
    pub(crate) reason: HibernationReason,
    /// The memory of the process tree of the command before the stop.
    /// `None` if the daemon could not measure it.
    pub(crate) freed: Option<(u64, MemoryMeasure)>,
    pub(crate) resume_command: String,
    /// Seconds after the Unix epoch. `None` while the session is
    /// hibernated.
    pub(crate) woken_at: Option<u64>,
    /// Why the last wake failed.
    pub(crate) wake_error: Option<String>,
}

impl HibernationRecord {
    /// `true` while the session waits for its wake.
    pub(crate) fn is_hibernated(&self) -> bool {
        self.woken_at.is_none()
    }
}

/// The record in `session_directory`. `None` if the session has none, or
/// if the file cannot be read: then the session is not hibernated.
pub(super) fn read(session_directory: &Path) -> Option<HibernationRecord> {
    let bytes = small_file::read(&session_directory.join(FILE), MAX_LENGTH).ok()?;
    serde_json::from_slice(&bytes).ok()
}

pub(super) fn write(session_directory: &Path, record: &HibernationRecord) -> io::Result<()> {
    let bytes = serde_json::to_vec(record).map_err(io::Error::other)?;
    small_file::replace(session_directory, FILE, &bytes)
}

pub(super) fn remove(session_directory: &Path) -> io::Result<()> {
    small_file::remove(&session_directory.join(FILE))
}
