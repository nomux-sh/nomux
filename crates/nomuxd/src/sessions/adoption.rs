//! Takes over what the daemon finds at its start: the holders of an earlier
//! daemon, and the sessions of an earlier start of the host that have only
//! a state directory.

use std::collections::HashSet;
use std::io;

use nomux_core::configuration::RestorePolicy;
use nomux_holder_protocol::SessionId;
use nomux_holder_protocol::discovery::{HolderState, scan};
use tracing::{info, warn};

use super::Sessions;
use crate::holder;
use crate::session_registry::SessionState;

impl Sessions {
    /// Finds the sessions that exist. A session with a holder that runs is
    /// connected. A session that has only a state directory is dormant,
    /// ended or damaged: the daemon starts no process for it. The daemon
    /// takes no pooled holder of an earlier daemon.
    ///
    /// # Errors
    ///
    /// Returns an error if the runtime directory cannot be read. A state
    /// directory that cannot be read stops nothing.
    pub(crate) async fn adopt_existing(&self) -> io::Result<()> {
        self.remove_pooled_holders_of_earlier_daemon()?;
        let with_holder = self.adopt_holders().await?;
        let stored = self.store.session_ids().unwrap_or_else(|error| {
            warn!(%error, "cannot read the session state directories");
            Vec::new()
        });
        for session_id in stored {
            if !with_holder.contains(&session_id) {
                self.stopped.classify(&session_id);
            }
        }
        Ok(())
    }

    /// Starts the command of each dormant session that has the restore
    /// policy `auto`.
    pub(crate) async fn restore_automatic(&self) {
        let dormant = self
            .registry
            .records()
            .into_iter()
            .filter(|record| record.state == SessionState::Dormant)
            .filter(|record| self.restore_policy(&record.id) == RestorePolicy::Auto)
            // A hibernated agent waits for the user: it gave its memory back.
            .filter(|record| {
                !self
                    .store
                    .hibernation(&record.id)
                    .is_some_and(|hibernation| hibernation.is_hibernated())
            });
        for record in dormant {
            if let Err(error) = self.restore(&record.id, None).await {
                warn!(session = %record.id, %error, "cannot start the session again");
            }
        }
    }

    /// Connects to the holders that an earlier daemon left, and removes the
    /// directories of the ones that do not run. Returns the sessions that
    /// have a holder.
    async fn adopt_holders(&self) -> io::Result<HashSet<SessionId>> {
        let mut with_holder = HashSet::new();
        for holder in scan(&self.runtime_directory)? {
            let link = match holder.state {
                HolderState::Stale(reason) => {
                    info!(session = %holder.session_id, ?reason, "removing a stale holder directory");
                    if let Err(error) = holder.remove_directory() {
                        warn!(session = %holder.session_id, %error, "cannot remove a stale holder directory");
                    }
                    continue;
                }
                HolderState::Starting => None,
                HolderState::Live => holder::open_with_lease(
                    &holder.socket_path(),
                    &holder.session_id,
                )
                .await
                .inspect_err(|error| {
                    warn!(session = %holder.session_id, %error, "cannot connect to a holder");
                })
                .ok(),
            };
            with_holder.insert(holder.session_id.clone());
            self.supervise(holder.session_id, link);
        }
        Ok(with_holder)
    }
}
