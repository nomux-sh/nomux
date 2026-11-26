//! The sessions that the daemon knows, and their state.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use nomux_holder_protocol::SessionId;

use crate::holder::SessionEnd;
use crate::session_handle::SessionHandle;

/// The state of a session, as the daemon knows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionState {
    /// The daemon has a connection to the holder and the command runs.
    Running {
        /// The process ID of the command.
        pid: u32,
    },
    /// The holder runs, but the daemon has no connection to it.
    Unreachable,
    /// The session has no holder, and its command did not end. Its state
    /// directory has what a restore needs.
    Dormant,
    /// The command ended. The session stays in the list for the retention
    /// time.
    Ended(EndedSession),
    /// The session has no holder, and its metadata cannot be read.
    Damaged,
}

impl SessionState {
    /// `true` if the session has a holder.
    pub(crate) fn is_live(self) -> bool {
        matches!(self, Self::Running { .. } | Self::Unreachable)
    }
}

/// How and when the command of a session ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EndedSession {
    pub(crate) end: SessionEnd,
    /// Seconds after the Unix epoch.
    pub(crate) ended_at: u64,
}

/// One session at one moment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SessionRecord {
    pub(crate) id: SessionId,
    pub(crate) state: SessionState,
    /// Why a part of the state files of the session cannot be read.
    pub(crate) damage: Option<String>,
}

struct Entry {
    state: SessionState,
    damage: Option<String>,
    /// The ways into the session. Only a session with a holder has them.
    handle: Option<Arc<SessionHandle>>,
}

/// The list of the sessions. The task of each holder keeps the state of its
/// session current, and the request handlers read the list.
#[derive(Default)]
pub(crate) struct SessionRegistry {
    entries: Mutex<HashMap<SessionId, Entry>>,
}

impl SessionRegistry {
    /// Puts a session into the list, or replaces the session with the same
    /// identifier. A session without a holder has no handle.
    pub(crate) fn put(&self, record: SessionRecord, handle: Option<Arc<SessionHandle>>) {
        let entry = Entry {
            state: record.state,
            damage: record.damage,
            handle,
        };
        self.entries().insert(record.id, entry);
    }

    pub(crate) fn set_state(&self, id: &SessionId, state: SessionState) {
        if let Some(entry) = self.entries().get_mut(id) {
            entry.state = state;
        }
    }

    pub(crate) fn remove(&self, id: &SessionId) {
        self.entries().remove(id);
    }

    /// Removes the session `id` if it still has the state `state`. Returns
    /// `true` if it removed the session.
    pub(crate) fn remove_in_state(&self, id: &SessionId, state: SessionState) -> bool {
        let mut entries = self.entries();
        let unchanged = entries.get(id).is_some_and(|entry| entry.state == state);
        if unchanged {
            entries.remove(id);
        }
        unchanged
    }

    pub(crate) fn contains(&self, id: &SessionId) -> bool {
        self.entries().contains_key(id)
    }

    /// The sessions in the sequence of their identifiers.
    pub(crate) fn records(&self) -> Vec<SessionRecord> {
        let mut records: Vec<SessionRecord> = self
            .entries()
            .iter()
            .map(|(id, entry)| entry.record(id))
            .collect();
        records.sort_by(|a, b| a.id.as_str().cmp(b.id.as_str()));
        records
    }

    /// The session `id` and, if it has a holder, the ways into it.
    pub(crate) fn find(
        &self,
        id: &SessionId,
    ) -> Option<(SessionRecord, Option<Arc<SessionHandle>>)> {
        let entries = self.entries();
        let entry = entries.get(id)?;
        Some((entry.record(id), entry.handle.clone()))
    }

    fn entries(&self) -> MutexGuard<'_, HashMap<SessionId, Entry>> {
        // No code panics while it holds the lock, thus the map is whole.
        self.entries.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Entry {
    fn record(&self, id: &SessionId) -> SessionRecord {
        SessionRecord {
            id: id.clone(),
            state: self.state,
            damage: self.damage.clone(),
        }
    }
}
