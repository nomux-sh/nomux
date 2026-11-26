//! The task that owns the connection with the lease to one holder.

use std::collections::VecDeque;
use std::error::Error;
use std::fmt;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use nomux_holder_protocol::SessionId;
use nomux_holder_protocol::discovery::{DiscoveredHolder, HolderState, scan};
use nomux_holder_protocol::wire::{
    ChildSignal, ChildStatus, ErrorCode, MAX_PAYLOAD_LENGTH, Reply, Request, WindowSize,
};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::{Instant, sleep, sleep_until};
use tracing::{debug, error, info, warn};

use super::backoff::Backoff;
use super::connection::{HolderConnection, HolderLink, open_with_lease};
use crate::session_registry::{SessionRegistry, SessionState};
use crate::shutdown::Shutdown;
use crate::stopped_sessions::StoppedSessions;

/// After `SIGHUP`, the command has this time to end. Then it gets `SIGKILL`.
const HANGUP_GRACE: Duration = Duration::from_secs(2);

/// The holder answers a `WaitOutput` request for this position only when the
/// command has ended: the ring buffer never gets there. Thus the daemon
/// learns the end of the command without a timer.
const NEVER_WRITTEN: u64 = u64::MAX;

/// The answer to a command that writes to the holder.
pub(crate) type WriteDone = oneshot::Sender<Result<(), WriteError>>;

/// What a request handler or an attached client asks the task to do. Only
/// this task can write to the holder, because its connection has the lease.
pub(crate) enum SessionCommand {
    /// End the session. The task answers when the command has ended.
    Kill { done: WriteDone },
    /// Write bytes to the terminal. With `done`, the task answers when the
    /// holder has the bytes.
    Input {
        bytes: Vec<u8>,
        done: Option<WriteDone>,
    },
    /// Change the size of the terminal window.
    Resize { size: WindowSize, done: WriteDone },
}

/// Why the holder did not do a command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WriteError {
    /// A different daemon took the single-writer lease of the holder.
    LeaseLost,
    /// The holder refused the request.
    Refused(ErrorCode),
}

impl fmt::Display for WriteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LeaseLost => {
                formatter.write_str("a different daemon has the lease of the session")
            }
            Self::Refused(ErrorCode::ChildExited) => {
                formatter.write_str("the command of the session has ended")
            }
            Self::Refused(code) => write!(formatter, "the holder refused the request: {code:?}"),
        }
    }
}

impl Error for WriteError {}

/// How a session ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionEnd {
    /// The command ended with an exit code.
    Exited { code: i32 },
    /// A signal ended the command.
    Signaled { signal: i32 },
    /// The holder is gone, and the daemon did not get the exit status.
    Lost,
}

/// How one connection to the holder ended.
enum Served {
    SessionEnded(SessionEnd),
    /// A different daemon took the single-writer lease.
    LeaseLost,
    ConnectionLost(io::Error),
}

/// What the reply to a request that is with the holder means.
enum Expected {
    /// `WaitOutput`: the command has ended.
    EndOfCommand,
    Status,
    /// A signal for a kill request.
    Signal,
    /// `Input` or `Resize`.
    Write(Option<WriteDone>),
}

/// Owns the connection to the holder of one session for the life of the
/// session: it watches for the end of the command, writes to the holder on
/// request, and connects again after the connection is lost.
pub(crate) struct Supervisor {
    session_id: SessionId,
    runtime_directory: PathBuf,
    registry: Arc<SessionRegistry>,
    /// Takes the session when its holder is gone.
    stopped: Arc<StoppedSessions>,
    shutdown: Shutdown,
    commands: mpsc::Receiver<SessionCommand>,
    /// Tells the attached clients how the session ended.
    ended: watch::Sender<Option<SessionEnd>>,
    /// The requests that wait for the end of the session.
    kill_waiters: Vec<WriteDone>,
    /// When the command gets `SIGKILL`, after a kill request.
    force_kill_at: Option<Instant>,
    /// The requests of the current connection that have no reply yet.
    in_flight: VecDeque<(u32, Expected)>,
}

impl Supervisor {
    pub(crate) fn new(
        session_id: SessionId,
        runtime_directory: PathBuf,
        registry: Arc<SessionRegistry>,
        stopped: Arc<StoppedSessions>,
        shutdown: Shutdown,
        commands: mpsc::Receiver<SessionCommand>,
        ended: watch::Sender<Option<SessionEnd>>,
    ) -> Self {
        Self {
            session_id,
            runtime_directory,
            registry,
            stopped,
            shutdown,
            commands,
            ended,
            kill_waiters: Vec::new(),
            force_kill_at: None,
            in_flight: VecDeque::new(),
        }
    }

    /// Runs until the holder of the session is gone. Without a link, the
    /// task connects to the holder first.
    pub(crate) async fn run(mut self, mut link: Option<HolderLink>) {
        let end = loop {
            let current = match link.take() {
                Some(link) => link,
                None => match self.reconnect().await {
                    Some(link) => link,
                    None => break SessionEnd::Lost,
                },
            };
            let state = SessionState::Running {
                pid: current.hello.child_pid,
            };
            self.registry.set_state(&self.session_id, state);
            match self.serve(current.connection).await {
                Served::SessionEnded(end) => break end,
                Served::LeaseLost => return self.give_way_to_new_daemon(),
                Served::ConnectionLost(error) => {
                    warn!(session = %self.session_id, %error, "the connection to the holder is lost");
                    self.registry
                        .set_state(&self.session_id, SessionState::Unreachable);
                }
            }
        };
        info!(session = %self.session_id, ?end, "the holder of the session is gone");
        self.hand_over(end);
        self.ended.send_replace(Some(end));
        self.answer_kill_waiters(Ok(()));
    }

    /// Gives the session, which has no holder now, to the list of the
    /// stopped sessions.
    fn hand_over(&self, end: SessionEnd) {
        match end {
            // A client asked for the end of the session, thus the session
            // does not stay as a dormant session.
            SessionEnd::Lost if !self.kill_waiters.is_empty() => {
                self.stopped.forget(&self.session_id);
            }
            // The command did not end: the state directory says what the
            // session is now.
            SessionEnd::Lost => self.stopped.classify(&self.session_id),
            SessionEnd::Exited { .. } | SessionEnd::Signaled { .. } => {
                self.stopped.command_ended(&self.session_id, end);
            }
        }
    }

    /// Only one daemon can write to a holder. The daemon that asked for the
    /// lease last has it, thus this daemon stops.
    fn give_way_to_new_daemon(mut self) {
        error!(session = %self.session_id, "a different daemon took the lease of the holder; this daemon stops");
        self.answer_kill_waiters(Err(WriteError::LeaseLost));
        self.shutdown.trigger();
    }

    async fn serve(&mut self, mut connection: HolderConnection) -> Served {
        // The requests of an earlier connection get no reply. Their waiters
        // learn that when their channel closes.
        self.in_flight.clear();
        match self.converse(&mut connection).await {
            Ok(served) => served,
            Err(error) => Served::ConnectionLost(error),
        }
    }

    async fn converse(&mut self, connection: &mut HolderConnection) -> io::Result<Served> {
        self.request(connection, wait_for_end(), Expected::EndOfCommand)
            .await?;
        // A kill request from before a lost connection continues here.
        if !self.kill_waiters.is_empty() {
            self.hang_up(connection).await?;
        }
        loop {
            tokio::select! {
                reply = connection.receive() => {
                    let (request_id, reply) = reply?;
                    if let Some(served) = self.handle_reply(connection, request_id, reply).await? {
                        return Ok(served);
                    }
                }
                Some(command) = self.commands.recv() => self.handle_command(connection, command).await?,
                () = sleep_until(self.force_kill_at.unwrap_or_else(Instant::now)), if self.force_kill_at.is_some() => {
                    self.force_kill_at = None;
                    self.request(connection, Request::Signal(ChildSignal::Kill), Expected::Signal).await?;
                }
            }
        }
    }

    async fn handle_command(
        &mut self,
        connection: &mut HolderConnection,
        command: SessionCommand,
    ) -> io::Result<()> {
        match command {
            SessionCommand::Kill { done } => {
                if self.kill_waiters.is_empty() {
                    self.hang_up(connection).await?;
                }
                self.kill_waiters.push(done);
            }
            SessionCommand::Input { bytes, done } => {
                // The answer belongs to the last frame: the holder answers
                // the frames in their sequence.
                let mut frames = bytes.chunks(MAX_PAYLOAD_LENGTH).peekable();
                let mut done = done;
                while let Some(frame) = frames.next() {
                    let done = if frames.peek().is_none() {
                        done.take()
                    } else {
                        None
                    };
                    self.request(connection, Request::Input(frame), Expected::Write(done))
                        .await?;
                }
            }
            SessionCommand::Resize { size, done } => {
                self.request(
                    connection,
                    Request::Resize(size),
                    Expected::Write(Some(done)),
                )
                .await?;
            }
        }
        Ok(())
    }

    /// Does what the reply to an earlier request needs. Returns how the
    /// connection ended, if the reply ends it.
    async fn handle_reply(
        &mut self,
        connection: &mut HolderConnection,
        request_id: u32,
        reply: Reply,
    ) -> io::Result<Option<Served>> {
        let Some(index) = self.in_flight.iter().position(|(id, _)| *id == request_id) else {
            return Ok(None);
        };
        let Some((_, expected)) = self.in_flight.remove(index) else {
            return Ok(None);
        };
        match (expected, reply) {
            (Expected::Write(done), Reply::Error(ErrorCode::LeaseRequired)) => {
                answer(done, Err(WriteError::LeaseLost));
                return Ok(Some(Served::LeaseLost));
            }
            (_, Reply::Error(ErrorCode::LeaseRequired)) => return Ok(Some(Served::LeaseLost)),
            (Expected::EndOfCommand, Reply::Output { .. }) => {
                self.request(connection, Request::Status, Expected::Status)
                    .await?;
            }
            (Expected::Status, Reply::Status(ChildStatus::Running)) => {
                self.request(connection, wait_for_end(), Expected::EndOfCommand)
                    .await?;
            }
            (Expected::Status, Reply::Status(ChildStatus::Exited { code })) => {
                return Ok(Some(Served::SessionEnded(SessionEnd::Exited { code })));
            }
            (Expected::Status, Reply::Status(ChildStatus::Signaled { signal })) => {
                return Ok(Some(Served::SessionEnded(SessionEnd::Signaled { signal })));
            }
            // The command ended before the signal. The reply to `WaitOutput`
            // says so too.
            (Expected::Signal, Reply::Done | Reply::Error(ErrorCode::ChildExited)) => {}
            (Expected::Signal, Reply::Error(code)) => {
                warn!(session = %self.session_id, ?code, "the holder refused a signal");
                self.answer_kill_waiters(Err(WriteError::Refused(code)));
                self.force_kill_at = None;
            }
            (Expected::Write(done), Reply::Done) => answer(done, Ok(())),
            (Expected::Write(done), Reply::Error(code)) => {
                debug!(session = %self.session_id, ?code, "the holder refused a write");
                answer(done, Err(WriteError::Refused(code)));
            }
            (_, reply) => {
                warn!(session = %self.session_id, ?reply, "the holder sent a reply that does not answer its request");
            }
        }
        Ok(None)
    }

    /// Sends one request and records what its reply means.
    async fn request(
        &mut self,
        connection: &mut HolderConnection,
        request: Request<'_>,
        expected: Expected,
    ) -> io::Result<()> {
        let request_id = connection.send(request).await?;
        self.in_flight.push_back((request_id, expected));
        Ok(())
    }

    /// Sends `SIGHUP`, the signal that the end of a terminal gives to its
    /// processes. `SIGKILL` follows after the grace time.
    async fn hang_up(&mut self, connection: &mut HolderConnection) -> io::Result<()> {
        self.request(
            connection,
            Request::Signal(ChildSignal::Hangup),
            Expected::Signal,
        )
        .await?;
        self.force_kill_at = Some(Instant::now() + HANGUP_GRACE);
        Ok(())
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
                        match open_with_lease(&holder.socket_path(), &self.session_id).await {
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

    fn answer_kill_waiters(&mut self, result: Result<(), WriteError>) {
        for waiter in self.kill_waiters.drain(..) {
            answer(Some(waiter), result);
        }
    }
}

fn answer(done: Option<WriteDone>, result: Result<(), WriteError>) {
    if let Some(done) = done {
        // A waiter that is gone stopped waiting.
        let _ = done.send(result);
    }
}

fn wait_for_end() -> Request<'static> {
    Request::WaitOutput {
        after: NEVER_WRITTEN,
    }
}
