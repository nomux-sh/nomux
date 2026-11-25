//! Writes the session state directory of this holder: the directory of the
//! journal and the session metadata. The layout is in
//! `nomux_holder_protocol::session_state`.
//!
//! The holder never removes the directory. The daemon reads it after the
//! holder ended, and removes it when it forgets the session.

use std::env;
use std::fs::{self, DirBuilder, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use nomux_holder_protocol::journal::segment_files;
use nomux_holder_protocol::session_state::{
    JOURNAL_DIRECTORY, METADATA_FILE, RestorePolicy, SessionMetadata, session_state_directory,
};
use nomux_holder_protocol::wire::{ChildStatus, WindowSize};

use crate::arguments::SessionArguments;

const OWNER_ONLY: u32 = 0o700;
const OWNER_READ_WRITE: u32 = 0o600;
/// The metadata goes to this file first. A rename then replaces the
/// metadata file in one step.
const NEW_METADATA_FILE: &str = "session.new";

pub(crate) struct SessionStateDirectory {
    path: PathBuf,
    metadata: SessionMetadata,
}

impl SessionStateDirectory {
    /// Makes the directory of the session if it does not exist, and writes
    /// the session metadata with the next epoch of the session.
    pub(crate) fn create(arguments: &SessionArguments) -> io::Result<Self> {
        let path = session_state_directory(&arguments.state_directory, &arguments.session_id);
        let journal_directory = path.join(JOURNAL_DIRECTORY);
        DirBuilder::new()
            .recursive(true)
            .mode(OWNER_ONLY)
            .create(&journal_directory)?;
        let working_directory = match &arguments.working_directory {
            Some(directory) => directory.clone(),
            None => env::current_dir()?,
        };
        let metadata = SessionMetadata {
            epoch: next_epoch(&path, &journal_directory)?,
            started: seconds_after_unix_epoch(),
            size: arguments.size,
            restore_policy: RestorePolicy::OnFocus,
            // The environment of the command never goes to the disk.
            profile: String::new(),
            working_directory,
            command: arguments.program.clone(),
            arguments: arguments.program_arguments.clone(),
            status: ChildStatus::Running,
            ended: None,
        };
        let directory = Self { path, metadata };
        directory.write_metadata()?;
        Ok(directory)
    }

    pub(crate) fn epoch(&self) -> u32 {
        self.metadata.epoch
    }

    pub(crate) fn journal_directory(&self) -> PathBuf {
        self.path.join(JOURNAL_DIRECTORY)
    }

    pub(crate) fn record_window_size(&mut self, size: WindowSize) -> io::Result<()> {
        self.metadata.size = size;
        self.write_metadata()
    }

    pub(crate) fn record_end(&mut self, status: ChildStatus) -> io::Result<()> {
        self.metadata.status = status;
        self.metadata.ended = Some(seconds_after_unix_epoch());
        self.write_metadata()
    }

    fn write_metadata(&self) -> io::Result<()> {
        let new_file = self.path.join(NEW_METADATA_FILE);
        OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(OWNER_READ_WRITE)
            .open(&new_file)?
            .write_all(self.metadata.encode().as_bytes())?;
        fs::rename(new_file, self.path.join(METADATA_FILE))
    }
}

/// The epoch of a new holder: one more than the last epoch that the session
/// used. The position of the output starts at 0 with each holder, thus each
/// holder of a session needs its own epoch.
///
/// The last epoch is in the metadata file and in the names of the segment
/// files. The two are read, because a person can change or remove each.
fn next_epoch(session_directory: &Path, journal_directory: &Path) -> io::Result<u32> {
    let recorded = fs::read_to_string(session_directory.join(METADATA_FILE))
        .ok()
        .and_then(|text| SessionMetadata::parse(&text).ok())
        .map_or(0, |metadata| metadata.epoch);
    let written = segment_files(journal_directory)?
        .last()
        .map_or(0, |file| file.name.epoch);
    recorded
        .max(written)
        .checked_add(1)
        .ok_or_else(|| io::Error::other("the session has no epoch left"))
}

fn seconds_after_unix_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}
