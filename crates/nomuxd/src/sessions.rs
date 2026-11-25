//! The operations on sessions: create, list, end, and take over the sessions
//! of an earlier daemon.

use std::env;
use std::error::Error;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use nomux_holder_protocol::SessionId;
use nomux_holder_protocol::discovery::{HolderState, SOCKET_FILE, holder_directory, scan};
use tokio::sync::{mpsc, oneshot};
use tokio::time::timeout;
use tracing::{info, warn};

use crate::holder::{self, HolderLink, KillRequest, LaunchError, Supervisor};
use crate::session_id;
use crate::session_registry::{SessionRecord, SessionRegistry, SessionState};
use crate::shutdown::Shutdown;

/// The longest time that a request waits for the end of a session. The task
/// of the holder sends `SIGKILL` earlier than this.
const KILL_TIMEOUT: Duration = Duration::from_secs(10);
/// One client can have a few kill requests for the same session open.
const KILL_REQUEST_QUEUE: usize = 8;
const FALLBACK_SHELL: &str = "/bin/sh";

/// Why an operation on a session did not succeed.
#[derive(Debug)]
pub(crate) enum SessionError {
    NotFound(String),
    Timeout(String),
    Failed(String),
}

impl fmt::Display for SessionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (Self::NotFound(message) | Self::Timeout(message) | Self::Failed(message)) = self;
        formatter.write_str(message)
    }
}

impl Error for SessionError {}

/// The sessions of the host.
pub(crate) struct Sessions {
    runtime_directory: PathBuf,
    registry: Arc<SessionRegistry>,
    shutdown: Shutdown,
}

impl Sessions {
    pub(crate) fn new(runtime_directory: PathBuf, shutdown: Shutdown) -> Self {
        Self {
            runtime_directory,
            registry: Arc::default(),
            shutdown,
        }
    }

    /// Finds the holders that an earlier daemon left. Connects to the live
    /// ones and removes the directories of the ones that do not run.
    pub(crate) async fn adopt_holders(&self) -> io::Result<()> {
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
                HolderState::Live => holder::open(&holder.socket_path(), &holder.session_id)
                    .await
                    .inspect_err(|error| {
                        warn!(session = %holder.session_id, %error, "cannot connect to a holder");
                    })
                    .ok(),
            };
            self.supervise(holder.session_id, link);
        }
        Ok(())
    }

    /// Starts a holder with `command` and connects to it. An empty command
    /// is the shell of the user. No working directory is the home directory
    /// of the user.
    pub(crate) async fn create(
        &self,
        command: Vec<String>,
        working_directory: Option<PathBuf>,
    ) -> Result<SessionRecord, SessionError> {
        let command = if command.is_empty() {
            vec![env::var("SHELL").unwrap_or_else(|_| FALLBACK_SHELL.to_owned())]
        } else {
            command
        };
        let working_directory = working_directory_of_session(working_directory)?;
        let session_id = self.unused_session_id().map_err(start_failure)?;

        holder::launch(
            &self.runtime_directory,
            &session_id,
            &command,
            &working_directory,
        )
        .await
        .map_err(|error| match error {
            LaunchError::NotReady => SessionError::Timeout(error.to_string()),
            _ => start_failure(error),
        })?;
        let socket = holder_directory(&self.runtime_directory, &session_id).join(SOCKET_FILE);
        let link = holder::open(&socket, &session_id)
            .await
            .map_err(start_failure)?;
        Ok(self.supervise(session_id, Some(link)))
    }

    /// The sessions in the sequence of their identifiers.
    pub(crate) fn list(&self) -> Vec<SessionRecord> {
        self.registry.records()
    }

    /// Ends the session `id` and waits until its command has ended.
    pub(crate) async fn kill(&self, id: &str) -> Result<(), SessionError> {
        let not_found = || SessionError::NotFound(format!("no session has the identifier {id}"));
        // Text that is not a session identifier names no session.
        let session_id: SessionId = id.parse().map_err(|_| not_found())?;
        let kill_requests = self
            .registry
            .kill_requests(&session_id)
            .ok_or_else(not_found)?;

        let (done, ended) = oneshot::channel();
        // If the task of the session is gone, the session has ended.
        if kill_requests.send(KillRequest { done }).await.is_err() {
            return Ok(());
        }
        match timeout(KILL_TIMEOUT, ended).await {
            Ok(Ok(Ok(())) | Err(_)) => Ok(()),
            Ok(Ok(Err(error))) => Err(SessionError::Failed(format!(
                "session {id} did not end: {error}"
            ))),
            Err(_) => Err(SessionError::Timeout(format!(
                "session {id} did not end in {} seconds",
                KILL_TIMEOUT.as_secs()
            ))),
        }
    }

    /// Puts a session into the list and starts the task that owns the
    /// connection to its holder. Without a link, the task connects itself.
    fn supervise(&self, session_id: SessionId, link: Option<HolderLink>) -> SessionRecord {
        let state = match &link {
            Some(link) => SessionState::Running {
                pid: link.command_pid,
            },
            None => SessionState::Unreachable,
        };
        let (kill_requests, receiver) = mpsc::channel(KILL_REQUEST_QUEUE);
        self.registry
            .insert(session_id.clone(), state, kill_requests);
        let supervisor = Supervisor::new(
            session_id.clone(),
            self.runtime_directory.clone(),
            Arc::clone(&self.registry),
            self.shutdown.clone(),
            receiver,
        );
        tokio::spawn(supervisor.run(link));
        SessionRecord {
            id: session_id,
            state,
        }
    }

    /// A random identifier that no session and no holder directory has.
    fn unused_session_id(&self) -> io::Result<SessionId> {
        loop {
            let session_id = session_id::random()?;
            let in_use = self.registry.contains(&session_id)
                || holder_directory(&self.runtime_directory, &session_id).exists();
            if !in_use {
                return Ok(session_id);
            }
        }
    }
}

fn working_directory_of_session(requested: Option<PathBuf>) -> Result<PathBuf, SessionError> {
    let directory = match requested {
        Some(directory) => directory,
        None => env::var_os("HOME").map(PathBuf::from).ok_or_else(|| {
            SessionError::Failed("the HOME variable of the daemon is not set".to_owned())
        })?,
    };
    if !is_absolute_directory(&directory) {
        return Err(SessionError::Failed(format!(
            "the working directory {} is not an absolute path of a directory",
            directory.display()
        )));
    }
    Ok(directory)
}

fn is_absolute_directory(path: &Path) -> bool {
    path.is_absolute() && path.is_dir()
}

fn start_failure(error: impl fmt::Display) -> SessionError {
    SessionError::Failed(format!("the session did not start: {error}"))
}
