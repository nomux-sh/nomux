//! The names of the sessions: a new session with a name, a rename, and the
//! rule that a name is unique among the sessions that are not ended.
//!
//! The name of a session is in its state directory (`SessionStore::name`).
//! One lock covers each check and each change of a name, thus of two
//! requests for one name, one gets it.

use std::path::PathBuf;
use std::sync::Arc;

use nomux_holder_protocol::SessionId;
use nomux_holder_protocol::discovery::pooled_holder_directory;

use super::{ListedSession, Sessions};
use crate::session_error::SessionError;
use crate::session_name::SessionName;
use crate::session_registry::{SessionRecord, SessionState};

/// A session that `Sessions::create_named` gives.
pub(crate) struct NamedSession {
    pub(crate) session: ListedSession,
    /// `false` if the session existed and the request asked for it.
    pub(crate) created: bool,
}

impl Sessions {
    /// Starts a session with `command` and `name`. If a session that is not
    /// ended has the name: with `reuse_existing` the result is that session,
    /// else the request fails.
    pub(crate) async fn create_named(
        self: &Arc<Self>,
        command: Vec<String>,
        working_directory: Option<PathBuf>,
        sandbox: Option<&str>,
        name: SessionName,
        reuse_existing: bool,
    ) -> Result<NamedSession, SessionError> {
        let _naming = self.naming.lock().await;
        if let Some(owner) = self.session_named(&name, None) {
            return if reuse_existing {
                Ok(NamedSession {
                    session: self.describe(owner),
                    created: false,
                })
            } else {
                Err(SessionError::Failed(format!(
                    "the name {name} is in use by session {}. Run `nomux new -A -n {name}` to \
                     attach to it, or give a different name",
                    owner.id
                )))
            };
        }
        self.refuse_identifier(&name, None)?;
        let created = self.create(command, working_directory, sandbox).await?;
        let id = created.record.id.clone();
        if let Err(error) = self.store.set_name(&id, Some(&name)) {
            // A session without the name that the client asked for is not
            // the session that it asked for.
            let _ = self.kill(id.as_str()).await;
            return Err(SessionError::Failed(format!(
                "the name of the new session cannot be stored: {error}"
            )));
        }
        Ok(NamedSession {
            session: self.describe(created.record),
            created: true,
        })
    }

    /// Gives the session `id` the name `name`, or removes its name for
    /// `None`.
    pub(crate) async fn rename(
        &self,
        id: &str,
        name: Option<SessionName>,
    ) -> Result<ListedSession, SessionError> {
        let _naming = self.naming.lock().await;
        let (record, _) = self.find(id)?;
        if let Some(name) = &name {
            if let Some(owner) = self.session_named(name, Some(&record.id)) {
                return Err(SessionError::Failed(format!(
                    "the name {name} is in use by session {}",
                    owner.id
                )));
            }
            self.refuse_identifier(name, Some(&record.id))?;
        }
        self.store
            .set_name(&record.id, name.as_ref())
            .map_err(|error| {
                SessionError::Failed(format!(
                    "the name of session {id} cannot be stored: {error}"
                ))
            })?;
        Ok(self.describe(record))
    }

    /// `true` if a session that is not ended has the identifier `id` as its
    /// name. A new identifier must not be such a name.
    pub(super) fn is_name_in_use(&self, id: &SessionId) -> bool {
        id.as_str()
            .parse()
            .is_ok_and(|name| self.session_named(&name, None).is_some())
    }

    /// The session that is not ended, is not `except` and has `name`.
    fn session_named(
        &self,
        name: &SessionName,
        except: Option<&SessionId>,
    ) -> Option<SessionRecord> {
        self.registry.records().into_iter().find(|record| {
            is_other_live(record, except) && self.store.name(&record.id).as_ref() == Some(name)
        })
    }

    /// Fails if `name` is the identifier of a session that is not ended and
    /// is not `except`: a target would then name two sessions.
    fn refuse_identifier(
        &self,
        name: &SessionName,
        except: Option<&SessionId>,
    ) -> Result<(), SessionError> {
        let is_identifier = name.as_str().parse().is_ok_and(|id: SessionId| {
            // A pooled holder has the identifier of its next session.
            pooled_holder_directory(&self.runtime_directory, &id).exists()
                || self
                    .registry
                    .find(&id)
                    .is_some_and(|(record, _)| is_other_live(&record, except))
        });
        if is_identifier {
            Err(SessionError::Failed(format!(
                "the name {name} is the identifier of a different session"
            )))
        } else {
            Ok(())
        }
    }
}

fn is_other_live(record: &SessionRecord, except: Option<&SessionId>) -> bool {
    !matches!(record.state, SessionState::Ended(_)) && Some(&record.id) != except
}
