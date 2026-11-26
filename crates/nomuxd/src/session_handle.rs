//! The ways into one session: what a request handler or an attached client
//! holds after it found the session in the registry.
//!
//! Terminal data does not go through the registry. A client that attaches
//! takes the handle one time, and then it uses only the handle.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use nomux_holder_protocol::SessionId;
use nomux_holder_protocol::sandbox::FeatureSet;
use nomux_holder_protocol::wire::{ChildSignal, WindowSize};
use nomux_status::Timestamp;
use tokio::sync::{Notify, mpsc, oneshot, watch};
use tokio::time::timeout;

use crate::attachment::AttachedClients;
use crate::clock;
use crate::holder::{SessionCommand, SessionEnd, SharedOutputFeed, WriteError};
use crate::session_error::SessionError;

/// The longest time that a request waits for the end of a session. The task
/// of the holder sends `SIGKILL` earlier than this.
const KILL_TIMEOUT: Duration = Duration::from_secs(10);

pub(crate) struct SessionHandle {
    id: SessionId,
    /// The way to the task that writes to the holder.
    commands: mpsc::Sender<SessionCommand>,
    /// How the session ended. `None` while it runs.
    pub(crate) ended: watch::Receiver<Option<SessionEnd>>,
    /// The read side of the holder for the attached clients.
    pub(crate) output: SharedOutputFeed,
    pub(crate) clients: AttachedClients,
    activity: Mutex<Activity>,
    /// Tells the hibernation that a fact of its rules changed without an
    /// event: a client detached.
    hibernation_check: Arc<Notify>,
    /// The sandbox features that the holder applies to the command, from
    /// its last hello.
    sandbox: Mutex<FeatureSet>,
}

/// What clients did with the session.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Activity {
    /// When a client last wrote input.
    pub(crate) last_input: Option<Timestamp>,
    /// When the last client detached.
    pub(crate) last_detach: Option<Timestamp>,
}

impl SessionHandle {
    pub(crate) fn new(
        id: SessionId,
        commands: mpsc::Sender<SessionCommand>,
        ended: watch::Receiver<Option<SessionEnd>>,
        output: SharedOutputFeed,
        hibernation_check: Arc<Notify>,
    ) -> Self {
        Self {
            id,
            commands,
            ended,
            output,
            clients: AttachedClients::default(),
            activity: Mutex::default(),
            hibernation_check,
            sandbox: Mutex::default(),
        }
    }

    /// The sandbox features that apply to the command now.
    pub(crate) fn sandbox_features(&self) -> FeatureSet {
        *self.sandbox.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn set_sandbox_features(&self, features: FeatureSet) {
        *self.sandbox.lock().unwrap_or_else(PoisonError::into_inner) = features;
    }

    pub(crate) fn activity(&self) -> Activity {
        *self.activity.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// A client detached from the session.
    pub(crate) fn detached(&self) {
        self.activity
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .last_detach = Some(clock::now());
        self.hibernation_check.notify_one();
    }

    fn record_input(&self) {
        self.activity
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .last_input = Some(clock::now());
    }

    /// Writes a checkpoint of the screen, ends the command with `signals`
    /// and keeps the session as a dormant session. Waits until the command
    /// has ended.
    pub(crate) async fn hibernate(
        &self,
        signals: Vec<ChildSignal>,
        wait: Duration,
    ) -> Result<(), SessionError> {
        // Each signal has its wait, and the last one ends the command.
        let longest = wait * u32::try_from(signals.len()).unwrap_or(u32::MAX) + KILL_TIMEOUT;
        let (done, ended) = oneshot::channel();
        let command = SessionCommand::Hibernate {
            signals,
            wait,
            done,
        };
        if self.commands.send(command).await.is_err() {
            return Err(SessionError::Failed(format!(
                "session {} ended before its hibernation",
                self.id
            )));
        }
        match timeout(longest, ended).await {
            Ok(Ok(Ok(()))) => Ok(()),
            Ok(Ok(Err(error))) => Err(SessionError::Failed(format!(
                "session {} did not hibernate: {error}",
                self.id
            ))),
            Ok(Err(_)) => Err(SessionError::Failed(format!(
                "session {} ended before its hibernation",
                self.id
            ))),
            Err(_) => Err(SessionError::Timeout(format!(
                "the command of session {} did not end in {} seconds",
                self.id,
                longest.as_secs()
            ))),
        }
    }

    pub(crate) fn id(&self) -> &SessionId {
        &self.id
    }

    /// Ends the session and waits until its command has ended.
    pub(crate) async fn kill(&self) -> Result<(), SessionError> {
        let (done, ended) = oneshot::channel();
        // If the task of the session is gone, the session has ended.
        if self
            .commands
            .send(SessionCommand::Kill { done })
            .await
            .is_err()
        {
            return Ok(());
        }
        match timeout(KILL_TIMEOUT, ended).await {
            Ok(Ok(Ok(())) | Err(_)) => Ok(()),
            Ok(Ok(Err(error))) => Err(SessionError::Failed(format!(
                "session {} did not end: {error}",
                self.id
            ))),
            Err(_) => Err(SessionError::Timeout(format!(
                "session {} did not end in {} seconds",
                self.id,
                KILL_TIMEOUT.as_secs()
            ))),
        }
    }

    /// Changes the size of the terminal window of the session.
    pub(crate) async fn resize(&self, size: WindowSize) -> Result<(), SessionError> {
        let (done, resized) = oneshot::channel();
        self.write(SessionCommand::Resize { size, done }, resized)
            .await
            .map_err(|reason| {
                SessionError::Failed(format!(
                    "the window size of session {} did not change: {reason}",
                    self.id
                ))
            })
    }

    /// Writes bytes to the terminal of the session and waits until the
    /// holder has them.
    pub(crate) async fn write_input(&self, bytes: Vec<u8>) -> Result<(), SessionError> {
        self.record_input();
        let (done, written) = oneshot::channel();
        let command = SessionCommand::Input {
            bytes,
            done: Some(done),
        };
        self.write(command, written).await.map_err(|reason| {
            SessionError::Failed(format!(
                "session {} did not get the input: {reason}",
                self.id
            ))
        })
    }

    /// Sends bytes to the terminal of the session and does not wait for the
    /// holder. The bytes are discarded if the session has ended.
    pub(crate) async fn forward_input(&self, bytes: Vec<u8>) {
        self.record_input();
        let command = SessionCommand::Input { bytes, done: None };
        // A closed channel: the task of the session is gone.
        let _ = self.commands.send(command).await;
    }

    async fn write(
        &self,
        command: SessionCommand,
        done: oneshot::Receiver<Result<(), WriteError>>,
    ) -> Result<(), String> {
        const SESSION_ENDED: &str = "the session ended or its holder does not answer";
        self.commands
            .send(command)
            .await
            .map_err(|_| SESSION_ENDED.to_owned())?;
        match done.await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => Err(error.to_string()),
            Err(_) => Err(SESSION_ENDED.to_owned()),
        }
    }
}
