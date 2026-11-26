//! The session state directories of the host: what each session keeps on
//! disk. `docs/holder.md` has the layout.
//!
//! The holder of a session writes its directory: the session metadata and
//! the journal. The daemon reads the two, also when no holder runs, and it
//! owns four files of its own in the directory: the restore policy, the
//! name, the last hibernation and the pin. Only the daemon removes a
//! directory.
//!
//! The daemon trusts no file of a directory. A file that is damaged, too
//! long or not a regular file gives a reason for a person, and it never
//! stops the daemon.

mod hibernation_file;
mod journal_check;
mod name_file;
mod pin_file;
mod restore_policy_file;
pub(crate) mod small_file;
mod stored_metadata;

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::session_name::SessionName;
use nomux_core::configuration::RestorePolicy;
use nomux_holder_protocol::SessionId;
use nomux_holder_protocol::session_state::{
    JOURNAL_DIRECTORY, METADATA_FILE, SessionMetadata, default_state_directory,
    session_state_directory,
};

pub(crate) use self::hibernation_file::{HibernationReason, HibernationRecord};

/// The name that `session_state_directory` of the holder contract gives to
/// the directory with one directory for each session. A test compares the
/// two.
const SESSIONS_DIRECTORY: &str = "sessions";

pub(crate) struct SessionStore {
    state_directory: PathBuf,
}

impl SessionStore {
    /// The store in the state directory of the user.
    pub(crate) fn locate() -> io::Result<Self> {
        let state_directory = default_state_directory().ok_or_else(|| {
            io::Error::other("the HOME variable is not set, thus the state directory has no path")
        })?;
        Ok(Self { state_directory })
    }

    /// The directory that a holder gets as its `--state-directory`.
    pub(crate) fn state_directory(&self) -> &Path {
        &self.state_directory
    }

    /// The identifiers of the sessions that have a state directory. An
    /// entry whose name is not a session identifier is not a session.
    pub(crate) fn session_ids(&self) -> io::Result<Vec<SessionId>> {
        let entries = match fs::read_dir(self.sessions_directory()) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        let mut ids = Vec::new();
        for entry in entries {
            let name = entry?.file_name();
            if let Some(id) = name.to_str().and_then(|name| name.parse().ok()) {
                ids.push(id);
            }
        }
        Ok(ids)
    }

    pub(crate) fn has(&self, id: &SessionId) -> bool {
        self.session_directory(id).is_dir()
    }

    /// The session metadata that the holder of `id` wrote.
    ///
    /// # Errors
    ///
    /// Returns the reason, for a person, why the file cannot be used.
    pub(crate) fn metadata(&self, id: &SessionId) -> Result<SessionMetadata, String> {
        stored_metadata::read(&self.session_directory(id).join(METADATA_FILE))
    }

    pub(crate) fn journal_directory(&self, id: &SessionId) -> PathBuf {
        self.session_directory(id).join(JOURNAL_DIRECTORY)
    }

    /// Why a part of the journal of `id` cannot be read, if the header of a
    /// segment file says so. Damage inside a segment is found only when the
    /// output is read.
    pub(crate) fn journal_damage(&self, id: &SessionId) -> Option<String> {
        journal_check::damage(&self.journal_directory(id))
    }

    /// The restore policy that a client gave to `id`. `None` if the session
    /// has none.
    pub(crate) fn restore_policy(&self, id: &SessionId) -> Option<RestorePolicy> {
        restore_policy_file::read(&self.session_directory(id))
    }

    pub(crate) fn set_restore_policy(
        &self,
        id: &SessionId,
        policy: RestorePolicy,
    ) -> io::Result<()> {
        restore_policy_file::write(&self.session_directory(id), policy)
    }

    /// The name that a client gave to `id`. `None` if the session has none.
    pub(crate) fn name(&self, id: &SessionId) -> Option<SessionName> {
        name_file::read(&self.session_directory(id))
    }

    /// Gives `id` the name `name`, or removes its name for `None`.
    pub(crate) fn set_name(&self, id: &SessionId, name: Option<&SessionName>) -> io::Result<()> {
        name_file::write(&self.session_directory(id), name)
    }

    /// The last hibernation of `id`. `None` if the session never hibernated.
    pub(crate) fn hibernation(&self, id: &SessionId) -> Option<HibernationRecord> {
        hibernation_file::read(&self.session_directory(id))
    }

    pub(crate) fn set_hibernation(
        &self,
        id: &SessionId,
        record: &HibernationRecord,
    ) -> io::Result<()> {
        hibernation_file::write(&self.session_directory(id), record)
    }

    pub(crate) fn remove_hibernation(&self, id: &SessionId) -> io::Result<()> {
        hibernation_file::remove(&self.session_directory(id))
    }

    /// `true` if the user excluded `id` from automatic hibernation.
    pub(crate) fn is_pinned(&self, id: &SessionId) -> bool {
        pin_file::is_pinned(&self.session_directory(id))
    }

    pub(crate) fn set_pinned(&self, id: &SessionId, pinned: bool) -> io::Result<()> {
        pin_file::set(&self.session_directory(id), pinned)
    }

    /// Removes the state directory of `id`: the daemon forgets the session.
    pub(crate) fn remove(&self, id: &SessionId) -> io::Result<()> {
        match fs::remove_dir_all(self.session_directory(id)) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        }
    }

    fn session_directory(&self, id: &SessionId) -> PathBuf {
        session_state_directory(&self.state_directory, id)
    }

    /// The directory that has one directory for each session.
    fn sessions_directory(&self) -> PathBuf {
        self.state_directory.join(SESSIONS_DIRECTORY)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sessions_directory_is_the_one_of_the_holder_contract() {
        let store = SessionStore {
            state_directory: PathBuf::from("/state"),
        };
        let id: SessionId = "k3xq2abc".parse().unwrap();

        assert_eq!(
            store.session_directory(&id),
            store.sessions_directory().join(id.as_str())
        );
    }
}
