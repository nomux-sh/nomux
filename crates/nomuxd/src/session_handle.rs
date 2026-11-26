//! The ways into one session: what a request handler or an attached client
//! holds after it found the session in the registry.
//!
//! Terminal data does not go through the registry. A client that attaches
//! takes the handle one time, and then it uses only the handle.

use std::time::Duration;

use nomux_holder_protocol::SessionId;
use nomux_holder_protocol::wire::WindowSize;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::timeout;

use crate::attachment::AttachedClients;
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
}

impl SessionHandle {
    pub(crate) fn new(
        id: SessionId,
        commands: mpsc::Sender<SessionCommand>,
        ended: watch::Receiver<Option<SessionEnd>>,
        output: SharedOutputFeed,
    ) -> Self {
        Self {
            id,
            commands,
            ended,
            output,
            clients: AttachedClients::default(),
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
