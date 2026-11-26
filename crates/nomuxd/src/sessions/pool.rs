//! The holder pool of the sessions: its task, which starts the pooled
//! holders, and the take of a pooled holder for a new session.

use std::fs;
use std::io;
use std::path::Path;
use std::sync::Arc;

use nomux_holder_protocol::discovery::{
    HolderState, SOCKET_FILE, pooled_holder_directory, scan_pooled,
};
use tokio::time::sleep_until;
use tracing::{debug, info, warn};

use super::{Sessions, start_failure};
use crate::holder::{self, HolderStart};
use crate::holder_pool::{PoolStep, PooledHolder, ShellStart, remove_pool_directory};
use crate::session_error::SessionError;

impl Sessions {
    /// Starts the pool for the first request of a client after the start of
    /// the daemon.
    pub(crate) fn start_pool_for_first_request(self: &Arc<Self>) {
        if self.pool.start_for_first_request() {
            tokio::spawn(Arc::clone(self).run_pool());
        }
    }

    /// Starts the pool after a new session, or keeps it for its idle time.
    pub(super) fn start_pool(self: &Arc<Self>) {
        if self.pool.start() {
            tokio::spawn(Arc::clone(self).run_pool());
        }
    }

    /// A pooled holder that has the session `working_directory` now, with
    /// the identifier of the session. `None` if the pool has no holder that
    /// can take the session. The status registry knows the token of the
    /// holder before the assignment: the shell can run a command from then.
    pub(super) async fn take_pooled(&self, working_directory: &Path) -> Option<PooledHolder> {
        while let Some(mut holder) = self.pool.take(working_directory, &self.home) {
            self.status.starting(&holder.session_id, holder.token);
            let assigned = holder::assign(&mut holder.link, working_directory).await;
            match assigned {
                Ok(()) => return Some(holder),
                Err(error) => {
                    warn!(session = %holder.session_id, %error, "a pooled holder did not take a session");
                    // A holder that is still pooled ends when its pipe
                    // closes. It can have written a state directory.
                    let session_id = holder.session_id.clone();
                    drop(holder);
                    let _ = self.store.remove(&session_id);
                    self.status.remove(&session_id);
                }
            }
        }
        None
    }

    /// Removes the directories of the pooled holders that an earlier daemon
    /// left. The daemon takes no such holder: its shell has the environment
    /// of that daemon. A pooled holder ends by itself when the pipe of its
    /// daemon closes, thus a holder that still runs ends a moment later.
    pub(super) fn remove_pooled_holders_of_earlier_daemon(&self) -> io::Result<()> {
        remove_pool_directory(&self.runtime_directory)?;
        for pooled in scan_pooled(&self.runtime_directory)? {
            let HolderState::Stale(reason) = pooled.state else {
                debug!(session = %pooled.session_id, "a pooled holder of an earlier daemon ends");
                continue;
            };
            info!(session = %pooled.session_id, ?reason, "removing a stale pooled holder directory");
            if let Err(error) = pooled.remove_directory() {
                warn!(session = %pooled.session_id, %error, "cannot remove a stale pooled holder directory");
            }
        }
        Ok(())
    }

    /// The task of the pool: fills the pool one holder at a time, and ends
    /// when the pool empties after its idle time.
    async fn run_pool(self: Arc<Self>) {
        loop {
            match self.pool.next_step() {
                PoolStep::Fill => match self.start_pooled_holder().await {
                    Ok(holder) => self.pool.add(holder),
                    Err(error) => {
                        warn!(%error, "cannot start a pooled holder");
                        self.pool.fill_failed();
                    }
                },
                PoolStep::Wait(idle_end) => {
                    tokio::select! {
                        () = self.pool.changed() => {}
                        () = sleep_until(idle_end) => {}
                    }
                }
                PoolStep::Stop => {
                    debug!("the holder pool is empty");
                    return;
                }
            }
        }
    }

    async fn start_pooled_holder(&self) -> Result<PooledHolder, SessionError> {
        let shell = self
            .pool
            .shell()
            .ok_or_else(|| SessionError::Failed("the holder pool is off".to_owned()))?;
        let session_id = self.unused_session_id().map_err(start_failure)?;
        let command = [shell.to_owned()];
        let shell_start = self
            .pool
            .start_file_shell()
            .map(|shell| ShellStart::prepare(shell, &self.runtime_directory, &session_id))
            .transpose()
            .map_err(start_failure)?;
        let start = HolderStart {
            session_id: &session_id,
            command: &command,
            launcher: &[],
            working_directory: &self.home,
            size: None,
            pooled: true,
            shell_start: shell_start.as_ref(),
        };
        let token = holder::new_token().map_err(start_failure)?;
        let state_directory = self.store.state_directory();
        let launched = holder::launch(&self.runtime_directory, state_directory, start, token).await;
        if launched.is_err()
            && let Some(shell_start) = &shell_start
        {
            // A holder that did not start does not remove its named pipe.
            let _ = fs::remove_file(&shell_start.directory_fifo);
        }
        let pipe = launched
            .map_err(start_failure)?
            .ok_or_else(|| SessionError::Failed("the pooled holder has no pipe".to_owned()))?;
        let socket =
            pooled_holder_directory(&self.runtime_directory, &session_id).join(SOCKET_FILE);
        let link = holder::open_with_lease(&socket, &session_id)
            .await
            .map_err(start_failure)?;
        Ok(PooledHolder {
            session_id,
            link,
            token,
            _pipe: pipe,
        })
    }
}
