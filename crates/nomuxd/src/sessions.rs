//! The operations on sessions: create, list, find, and take over the sessions
//! of an earlier daemon.

use std::env;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use nomux_holder_protocol::SessionId;
use nomux_holder_protocol::discovery::{HolderState, SOCKET_FILE, holder_directory, scan};
use tokio::sync::{mpsc, watch};
use tracing::{info, warn};

use crate::holder::{self, HolderLink, LaunchError, SharedOutputFeed, Supervisor};
use crate::session_error::SessionError;
use crate::session_handle::SessionHandle;
use crate::session_id;
use crate::session_registry::{SessionRecord, SessionRegistry, SessionState};
use crate::shutdown::Shutdown;

/// The commands of the attached clients and of the request handlers that
/// wait for the task of one session. A sender waits when the queue is full,
/// thus input that the holder does not take uses no more memory.
const COMMAND_QUEUE: usize = 64;
const FALLBACK_SHELL: &str = "/bin/sh";

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
        let link = holder::open_with_lease(&self.holder_socket(&session_id), &session_id)
            .await
            .map_err(start_failure)?;
        Ok(self.supervise(session_id, Some(link)))
    }

    /// The sessions in the sequence of their identifiers.
    pub(crate) fn list(&self) -> Vec<SessionRecord> {
        self.registry.records()
    }

    /// The session `id`, in each state.
    pub(crate) fn find(&self, id: &str) -> Result<Arc<SessionHandle>, SessionError> {
        self.find_with_state(id).map(|(_, handle)| handle)
    }

    /// The session `id`, if the daemon has a connection to its holder. A
    /// request for the terminal of a session cannot wait for a holder that
    /// does not answer.
    pub(crate) fn find_reachable(&self, id: &str) -> Result<Arc<SessionHandle>, SessionError> {
        match self.find_with_state(id)? {
            (SessionState::Running { .. }, handle) => Ok(handle),
            (SessionState::Unreachable, _) => Err(SessionError::Failed(format!(
                "the daemon has no connection to the holder of session {id}"
            ))),
        }
    }

    fn find_with_state(
        &self,
        id: &str,
    ) -> Result<(SessionState, Arc<SessionHandle>), SessionError> {
        // Text that is not a session identifier names no session.
        id.parse()
            .ok()
            .and_then(|session_id: SessionId| self.registry.find(&session_id))
            .ok_or_else(|| SessionError::NotFound(format!("no session has the identifier {id}")))
    }

    /// Puts a session into the list and starts the task that owns the
    /// connection to its holder. Without a link, the task connects itself.
    fn supervise(&self, session_id: SessionId, link: Option<HolderLink>) -> SessionRecord {
        let state = match &link {
            Some(link) => SessionState::Running {
                pid: link.hello.child_pid,
            },
            None => SessionState::Unreachable,
        };
        let (commands, command_receiver) = mpsc::channel(COMMAND_QUEUE);
        let (ended_sender, ended) = watch::channel(None);
        let output = SharedOutputFeed::new(self.holder_socket(&session_id), session_id.clone());
        let handle = SessionHandle::new(session_id.clone(), commands, ended, output);
        self.registry
            .insert(session_id.clone(), state, Arc::new(handle));
        let supervisor = Supervisor::new(
            session_id.clone(),
            self.runtime_directory.clone(),
            Arc::clone(&self.registry),
            self.shutdown.clone(),
            command_receiver,
            ended_sender,
        );
        tokio::spawn(supervisor.run(link));
        SessionRecord {
            id: session_id,
            state,
        }
    }

    fn holder_socket(&self, session_id: &SessionId) -> PathBuf {
        holder_directory(&self.runtime_directory, session_id).join(SOCKET_FILE)
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
