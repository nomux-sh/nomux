//! The task that owns the connection to one holder.

use std::error::Error;
use std::fmt;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use nomux_holder_protocol::SessionId;
use nomux_holder_protocol::discovery::{DiscoveredHolder, HolderState, scan};
use nomux_holder_protocol::wire::{ChildSignal, ChildStatus, ErrorCode, Reply, Request};
use tokio::sync::{mpsc, oneshot};
use tokio::time::{Instant, sleep, sleep_until};
use tracing::{debug, error, info, warn};

use super::backoff::Backoff;
use super::connection::{HolderConnection, HolderLink, open};
use crate::session_registry::{SessionRegistry, SessionState};
use crate::shutdown::Shutdown;

/// After `SIGHUP`, the command has this time to end. Then it gets `SIGKILL`.
const HANGUP_GRACE: Duration = Duration::from_secs(2);

/// The holder answers a `WaitOutput` request for this position only when the
/// command has ended: the ring buffer never gets there. Thus the daemon
/// learns the end of the command without a timer.
const NEVER_WRITTEN: u64 = u64::MAX;

/// A request to end the session. The task answers on `done` when the command
/// has ended.
pub(crate) struct KillRequest {
    pub(crate) done: oneshot::Sender<Result<(), KillError>>,
}

/// Why a session did not end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KillError {
    /// A different daemon took the single-writer lease of the holder.
    LeaseLost,
    /// The holder did not send the signal.
    Refused(ErrorCode),
}

impl fmt::Display for KillError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LeaseLost => {
                formatter.write_str("a different daemon has the lease of the session")
            }
            Self::Refused(code) => write!(formatter, "the holder refused the signal: {code:?}"),
        }
    }
}

impl Error for KillError {}

/// How one connection to the holder ended.
enum Served {
    /// The command of the session has ended.
    SessionEnded,
    /// A different daemon took the single-writer lease.
    LeaseLost,
    ConnectionLost(io::Error),
}

/// Owns the connection to the holder of one session for the life of the
/// session: it watches for the end of the command, ends the session on
/// request, and connects again after the connection is lost.
pub(crate) struct Supervisor {
    session_id: SessionId,
    runtime_directory: PathBuf,
    registry: Arc<SessionRegistry>,
    shutdown: Shutdown,
    kill_requests: mpsc::Receiver<KillRequest>,
    /// The requests that wait for the end of the session.
    kill_waiters: Vec<oneshot::Sender<Result<(), KillError>>>,
}

impl Supervisor {
    pub(crate) fn new(
        session_id: SessionId,
        runtime_directory: PathBuf,
        registry: Arc<SessionRegistry>,
        shutdown: Shutdown,
        kill_requests: mpsc::Receiver<KillRequest>,
    ) -> Self {
        Self {
            session_id,
            runtime_directory,
            registry,
            shutdown,
            kill_requests,
            kill_waiters: Vec::new(),
        }
    }

    /// Runs until the session ends. Without a link, the task connects to
    /// the holder first.
    pub(crate) async fn run(mut self, mut link: Option<HolderLink>) {
        loop {
            let current = match link.take() {
                Some(link) => link,
                None => match self.reconnect().await {
                    Some(link) => link,
                    None => break,
                },
            };
            let state = SessionState::Running {
                pid: current.command_pid,
            };
            self.registry.set_state(&self.session_id, state);
            match self.serve(current.connection).await {
                Served::SessionEnded => break,
                Served::LeaseLost => return self.give_way_to_new_daemon(),
                Served::ConnectionLost(error) => {
                    warn!(session = %self.session_id, %error, "the connection to the holder is lost");
                    self.registry
                        .set_state(&self.session_id, SessionState::Unreachable);
                }
            }
        }
        info!(session = %self.session_id, "the session ended");
        self.registry.remove(&self.session_id);
        self.answer_kill_waiters(Ok(()));
    }

    /// Only one daemon can write to a holder. The daemon that asked for the
    /// lease last has it, thus this daemon stops.
    fn give_way_to_new_daemon(mut self) {
        error!(session = %self.session_id, "a different daemon took the lease of the holder; this daemon stops");
        self.answer_kill_waiters(Err(KillError::LeaseLost));
        self.shutdown.trigger();
    }

    async fn serve(&mut self, mut connection: HolderConnection) -> Served {
        match self.converse(&mut connection).await {
            Ok(served) => served,
            Err(error) => Served::ConnectionLost(error),
        }
    }

    async fn converse(&mut self, connection: &mut HolderConnection) -> io::Result<Served> {
        connection.send(wait_for_end()).await?;
        // A kill request from before a lost connection continues here.
        let mut force_at = if self.kill_waiters.is_empty() {
            None
        } else {
            Some(hang_up(connection).await?)
        };
        loop {
            tokio::select! {
                reply = connection.receive() => match reply? {
                    Reply::Output { .. } => connection.send(Request::Status).await?,
                    Reply::Status(ChildStatus::Running) => connection.send(wait_for_end()).await?,
                    Reply::Status(_) => return Ok(Served::SessionEnded),
                    Reply::Error(ErrorCode::LeaseRequired) => return Ok(Served::LeaseLost),
                    // The command ended before the signal. The reply to
                    // `WaitOutput` says so too. This task sends no `Redraw`
                    // request, thus it gets no `Redraw` reply.
                    Reply::Done | Reply::Redraw { .. } | Reply::Error(ErrorCode::ChildExited) => {}
                    Reply::Error(code) => {
                        warn!(session = %self.session_id, ?code, "the holder refused a request");
                        self.answer_kill_waiters(Err(KillError::Refused(code)));
                        force_at = None;
                    }
                },
                Some(request) = self.kill_requests.recv() => {
                    if self.kill_waiters.is_empty() {
                        force_at = Some(hang_up(connection).await?);
                    }
                    self.kill_waiters.push(request.done);
                }
                () = sleep_until(force_at.unwrap_or_else(Instant::now)), if force_at.is_some() => {
                    connection.send(Request::Signal(ChildSignal::Kill)).await?;
                    force_at = None;
                }
            }
        }
    }

    /// Connects to the holder again. The pause between two attempts grows,
    /// thus there is no tight loop. Returns `None` if the holder is gone.
    async fn reconnect(&self) -> Option<HolderLink> {
        let mut backoff = Backoff::new();
        loop {
            match self.find_holder() {
                Ok(None) => return None,
                Ok(Some(holder)) => match holder.state {
                    HolderState::Stale(reason) => {
                        info!(session = %self.session_id, ?reason, "the holder is gone");
                        if let Err(error) = holder.remove_directory() {
                            warn!(session = %self.session_id, %error, "cannot remove the holder directory");
                        }
                        return None;
                    }
                    HolderState::Starting => {}
                    HolderState::Live => {
                        match open(&holder.socket_path(), &self.session_id).await {
                            Ok(link) => return Some(link),
                            Err(error) => {
                                debug!(session = %self.session_id, %error, "cannot connect to the holder");
                            }
                        }
                    }
                },
                Err(error) => {
                    warn!(session = %self.session_id, %error, "cannot read the runtime directory");
                }
            }
            sleep(backoff.next_delay()).await;
        }
    }

    /// The discovery state of the holder of this session. `None` if its
    /// directory does not exist.
    fn find_holder(&self) -> io::Result<Option<DiscoveredHolder>> {
        let holders = scan(&self.runtime_directory)?;
        Ok(holders
            .into_iter()
            .find(|holder| holder.session_id == self.session_id))
    }

    fn answer_kill_waiters(&mut self, result: Result<(), KillError>) {
        for waiter in self.kill_waiters.drain(..) {
            // A waiter that is gone stopped waiting.
            let _ = waiter.send(result);
        }
    }
}

fn wait_for_end() -> Request<'static> {
    Request::WaitOutput {
        after: NEVER_WRITTEN,
    }
}

/// Sends `SIGHUP`, the signal that the end of a terminal gives to its
/// processes. Returns the time when `SIGKILL` follows.
async fn hang_up(connection: &mut HolderConnection) -> io::Result<Instant> {
    connection
        .send(Request::Signal(ChildSignal::Hangup))
        .await?;
    Ok(Instant::now() + HANGUP_GRACE)
}
