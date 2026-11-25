//! The sessions that the daemon knows, and their state.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use nomux_holder_protocol::SessionId;

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
}

/// One session at one moment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SessionRecord {
    pub(crate) id: SessionId,
    pub(crate) state: SessionState,
}

struct Entry {
    state: SessionState,
    handle: Arc<SessionHandle>,
}

/// The list of the sessions. The task of each holder keeps the state of its
/// session current, and the request handlers read the list.
#[derive(Default)]
pub(crate) struct SessionRegistry {
    entries: Mutex<HashMap<SessionId, Entry>>,
}

impl SessionRegistry {
    pub(crate) fn insert(&self, id: SessionId, state: SessionState, handle: Arc<SessionHandle>) {
        self.entries().insert(id, Entry { state, handle });
    }

    pub(crate) fn set_state(&self, id: &SessionId, state: SessionState) {
        if let Some(entry) = self.entries().get_mut(id) {
            entry.state = state;
        }
    }

    pub(crate) fn remove(&self, id: &SessionId) {
        self.entries().remove(id);
    }

    pub(crate) fn contains(&self, id: &SessionId) -> bool {
        self.entries().contains_key(id)
    }

    /// The sessions in the sequence of their identifiers.
    pub(crate) fn records(&self) -> Vec<SessionRecord> {
        let mut records: Vec<SessionRecord> = self
            .entries()
            .iter()
            .map(|(id, entry)| SessionRecord {
                id: id.clone(),
                state: entry.state,
            })
            .collect();
        records.sort_by(|a, b| a.id.as_str().cmp(b.id.as_str()));
        records
    }

    /// The state of the session `id` and the ways into it.
    pub(crate) fn find(&self, id: &SessionId) -> Option<(SessionState, Arc<SessionHandle>)> {
        let entries = self.entries();
        let entry = entries.get(id)?;
        Some((entry.state, Arc::clone(&entry.handle)))
    }

    fn entries(&self) -> MutexGuard<'_, HashMap<SessionId, Entry>> {
        // No code panics while it holds the lock, thus the map is whole.
        self.entries.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
