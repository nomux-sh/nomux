//! The working directory of each session with its symbolic links resolved.
//! A client finds the Project of a session with it: `spec/workspace.md`.
//!
//! The daemon resolves the directory of a session one time: when it
//! describes the session the first time, thus in the response of
//! `Session.Create` and at the first request after the daemon found the
//! session again. The resolved path then does not change.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use nomux_holder_protocol::SessionId;

#[derive(Default)]
pub(super) struct ResolvedDirectories {
    /// `None` for a directory that the daemon could not resolve.
    resolved: Mutex<HashMap<SessionId, Option<PathBuf>>>,
}

impl ResolvedDirectories {
    /// The resolved path of `working_directory`, the directory of the
    /// session `id`.
    pub(super) fn of(&self, id: &SessionId, working_directory: &Path) -> Option<PathBuf> {
        self.resolved()
            .entry(id.clone())
            .or_insert_with(|| fs::canonicalize(working_directory).ok())
            .clone()
    }

    /// Forgets each session that is not in `ids`.
    pub(super) fn keep_only(&self, ids: &[&SessionId]) {
        self.resolved().retain(|id, _| ids.contains(&id));
    }

    fn resolved(&self) -> MutexGuard<'_, HashMap<SessionId, Option<PathBuf>>> {
        // No code panics while it holds the lock, thus the map is whole.
        self.resolved.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
