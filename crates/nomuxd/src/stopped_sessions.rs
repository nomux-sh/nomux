//! The sessions without a holder: what the daemon keeps of them, and when
//! it forgets them.
//!
//! - A session whose command ended stays in the list, with its state
//!   directory and its kept Agent run, for the retention time. Then the
//!   daemon removes the three.
//! - A session whose holder is gone, and whose command did not end, is
//!   dormant. Nothing removes it but a request of a client.
//!
//! The daemon does not poll for the end of a retention time. Each ended
//! session has one timer, which wakes the daemon one time. A daemon without
//! an ended session has no timer.

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use nomux_holder_protocol::SessionId;
use nomux_holder_protocol::wire::ChildStatus;
use tokio::time::sleep;
use tracing::{info, warn};

use crate::holder::SessionEnd;
use crate::session_registry::{EndedSession, SessionRecord, SessionRegistry, SessionState};
use crate::session_store::SessionStore;
use crate::status::StatusRegistry;

pub(crate) struct StoppedSessions {
    registry: Arc<SessionRegistry>,
    store: Arc<SessionStore>,
    status: Arc<StatusRegistry>,
    /// How long an ended session stays.
    retention: Duration,
}

impl StoppedSessions {
    pub(crate) fn new(
        registry: Arc<SessionRegistry>,
        store: Arc<SessionStore>,
        status: Arc<StatusRegistry>,
        retention: Duration,
    ) -> Self {
        Self {
            registry,
            store,
            status,
            retention,
        }
    }

    /// Puts the session `id`, which has no holder, into the list as what
    /// its state directory says: dormant, ended or damaged. A session
    /// without a state directory leaves the list.
    pub(crate) fn classify(self: &Arc<Self>, id: &SessionId) {
        if !self.store.has(id) {
            self.registry.remove(id);
            return;
        }
        let (state, damage) = match self.store.metadata(id) {
            Err(reason) => (SessionState::Damaged, Some(reason)),
            Ok(metadata) => {
                let state = match metadata.status {
                    ChildStatus::Running => SessionState::Dormant,
                    ChildStatus::Exited { code } => {
                        ended(SessionEnd::Exited { code }, metadata.ended)
                    }
                    ChildStatus::Signaled { signal } => {
                        ended(SessionEnd::Signaled { signal }, metadata.ended)
                    }
                };
                (state, self.store.journal_damage(id))
            }
        };
        self.keep(id, state, damage);
    }

    /// Keeps the session `id`, whose command ended now, for the retention
    /// time.
    pub(crate) fn command_ended(self: &Arc<Self>, id: &SessionId, end: SessionEnd) {
        self.keep(id, ended(end, None), None);
    }

    /// Removes the session `id` and its state directory.
    pub(crate) fn forget(&self, id: &SessionId) {
        self.registry.remove(id);
        self.remove_state(id);
    }

    fn keep(self: &Arc<Self>, id: &SessionId, state: SessionState, damage: Option<String>) {
        let record = SessionRecord {
            id: id.clone(),
            state,
            damage,
        };
        self.registry.put(record, None);
        if let SessionState::Ended(ended) = state {
            self.forget_after_retention(id.clone(), ended);
        }
    }

    fn forget_after_retention(self: &Arc<Self>, id: SessionId, ended: EndedSession) {
        let remaining = ended
            .ended_at
            .saturating_add(self.retention.as_secs())
            .saturating_sub(seconds_after_unix_epoch());
        let sessions = Arc::clone(self);
        tokio::spawn(async move {
            sleep(Duration::from_secs(remaining)).await;
            // A session that started again with the same identifier stays.
            if sessions
                .registry
                .remove_in_state(&id, SessionState::Ended(ended))
            {
                info!(session = %id, "the retention time of the ended session is over");
                sessions.status.remove(&id);
                sessions.remove_state(&id);
            }
        });
    }

    fn remove_state(&self, id: &SessionId) {
        if let Err(error) = self.store.remove(id) {
            warn!(session = %id, %error, "cannot remove the state directory of the session");
        }
    }
}

/// `ended_at` is the time in the session metadata. Without it, the command
/// ended now.
fn ended(end: SessionEnd, ended_at: Option<u64>) -> SessionState {
    SessionState::Ended(EndedSession {
        end,
        ended_at: ended_at.unwrap_or_else(seconds_after_unix_epoch),
    })
}

fn seconds_after_unix_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}
