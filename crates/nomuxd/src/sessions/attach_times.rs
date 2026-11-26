//! When a client attached to each session the last time. A client attaches
//! to the most recent session when the user names none.
//!
//! The daemon keeps the times in its memory only. After a restart of the
//! daemon a session has no time until the next attach.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, PoisonError};

use nomux_holder_protocol::SessionId;

use crate::clock;

const MILLISECONDS_IN_SECOND: u64 = 1000;

#[derive(Default)]
pub(super) struct AttachTimes {
    /// Seconds after the Unix epoch.
    times: Mutex<HashMap<SessionId, u64>>,
}

impl AttachTimes {
    /// A client attached to the session `id` now.
    pub(super) fn stamp(&self, id: &SessionId) {
        let now = clock::now().0 / MILLISECONDS_IN_SECOND;
        self.times().insert(id.clone(), now);
    }

    /// When a client attached to the session `id` the last time. `None` if
    /// no client attached since the start of the daemon.
    pub(super) fn of(&self, id: &SessionId) -> Option<u64> {
        self.times().get(id).copied()
    }

    /// Forgets each session that is not in `ids`.
    pub(super) fn keep_only(&self, ids: &[&SessionId]) {
        self.times().retain(|id, _| ids.contains(&id));
    }

    fn times(&self) -> MutexGuard<'_, HashMap<SessionId, u64>> {
        // No code panics while it holds the lock, thus the map is whole.
        self.times.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
