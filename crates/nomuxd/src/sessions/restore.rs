//! Starts the command of a dormant session again.
//!
//! A restore starts a new holder with the command, the working directory
//! and the window size of the session metadata. The session keeps its
//! identifier and its state directory, and the new holder gives it the next
//! epoch. The processes of the earlier epoch do not come back: the journal
//! has their output.

use std::ffi::OsString;
use std::fs;
use std::io;
use std::sync::Arc;

use nomux_core::configuration::RestorePolicy;
use nomux_holder_protocol::SessionId;
use nomux_holder_protocol::discovery::holder_directory;
use nomux_holder_protocol::wire::WindowSize;

use super::{ListedSession, Sessions, damaged};
use crate::holder::HolderStart;
use crate::session_error::SessionError;
use crate::session_handle::SessionHandle;
use crate::session_registry::{SessionRecord, SessionState};

/// A session that a client can attach to.
pub(crate) struct Attachable {
    pub(crate) session: Arc<SessionHandle>,
    /// The epoch of the session before this request started its command
    /// again. `None` if the command ran already.
    pub(crate) restored_from: Option<u32>,
}

impl Sessions {
    /// Starts the command of the dormant session `id` again. A session
    /// whose command runs stays as it is.
    pub(crate) async fn start(&self, id: &str) -> Result<ListedSession, SessionError> {
        let (record, _) = self.find(id)?;
        let record = match record.state {
            SessionState::Dormant => self.restore(&record.id, None).await?.0,
            SessionState::Damaged => return Err(damaged(&record)),
            _ => record,
        };
        Ok(self.describe(record))
    }

    /// The session `id` for an attach. A dormant session starts again, with
    /// the window `size` of the client, if its restore policy permits that.
    pub(crate) async fn attachable(
        &self,
        id: &str,
        size: Option<WindowSize>,
    ) -> Result<Attachable, SessionError> {
        let (record, _) = self.find(id)?;
        let mut restored_from = None;
        if record.state == SessionState::Dormant {
            if self.restore_policy(&record.id) == RestorePolicy::Manual {
                return Err(SessionError::Failed(format!(
                    "session {id} is dormant and its restore policy is manual: run `nomux start \
                     {id}`"
                )));
            }
            restored_from = self.restore(&record.id, size).await?.1;
        }
        let session = self.find_reachable(id)?;
        // A session that started now has the size of the client already.
        if let Some(size) = size.filter(|_| restored_from.is_none()) {
            session.resize(size).await?;
        }
        Ok(Attachable {
            session,
            restored_from,
        })
    }

    /// Starts a holder for the dormant session `id`. Returns the session,
    /// and the epoch that it had before, if this call started the holder.
    pub(super) async fn restore(
        &self,
        id: &SessionId,
        size: Option<WindowSize>,
    ) -> Result<(SessionRecord, Option<u32>), SessionError> {
        let _one_at_a_time = self.restoring.lock().await;
        // A different client can have started the session during the wait.
        match self.registry.find(id) {
            Some((record, _)) if record.state == SessionState::Dormant => {}
            Some((record, _)) if record.state.is_live() => return Ok((record, None)),
            _ => return Err(not_dormant(id)),
        }
        let metadata = self.store.metadata(id).map_err(|reason| {
            SessionError::Failed(format!("session {id} cannot start again: {reason}"))
        })?;
        self.remove_holder_directory(id).map_err(|error| {
            SessionError::Failed(format!(
                "session {id} cannot start again: the directory of its earlier holder cannot \
                 be removed: {error}"
            ))
        })?;
        let mut command: Vec<OsString> = vec![metadata.command];
        command.extend(metadata.arguments);
        let start = HolderStart {
            session_id: id,
            command: &command,
            working_directory: &metadata.working_directory,
            size: Some(size.unwrap_or(metadata.size)),
            pooled: false,
            shell_start: None,
        };
        let link = self.start_holder(start).await?;
        Ok((self.supervise(id.clone(), Some(link)), Some(metadata.epoch)))
    }

    /// A holder that was killed leaves its discovery directory, and a new
    /// holder does not start while the directory exists.
    fn remove_holder_directory(&self, id: &SessionId) -> io::Result<()> {
        match fs::remove_dir_all(holder_directory(&self.runtime_directory, id)) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        }
    }
}

fn not_dormant(id: &SessionId) -> SessionError {
    SessionError::NotFound(format!("no dormant session has the identifier {id}"))
}
