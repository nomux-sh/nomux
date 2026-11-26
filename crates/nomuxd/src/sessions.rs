//! The operations on sessions: create, list and find. The child modules
//! take over the sessions of an earlier daemon and of an earlier start of
//! the host, and start a dormant session again.

mod adoption;
mod memory;
mod pool;
mod resolved_directories;
mod restore;

use std::env;
use std::ffi::OsString;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use nomux_core::configuration::RestorePolicy;
use nomux_holder_protocol::SessionId;
use nomux_holder_protocol::discovery::{SOCKET_FILE, holder_directory, pooled_holder_directory};
use nomux_holder_protocol::session_state::SessionMetadata;
use tokio::sync::{Mutex, mpsc, watch};

use crate::holder::{
    self, HolderLink, HolderStart, LaunchError, OutputFeed, SharedOutputFeed, Supervisor,
};
use crate::holder_pool::HolderPool;
use crate::session_error::SessionError;
use crate::session_handle::SessionHandle;
use crate::session_id;
use crate::session_journal::SessionJournal;
use crate::session_registry::{SessionRecord, SessionRegistry, SessionState};
use crate::session_store::SessionStore;
use crate::settings::Settings;
use crate::shutdown::Shutdown;
use crate::status::{self, StatusRegistry};
use crate::stopped_sessions::StoppedSessions;

use self::resolved_directories::ResolvedDirectories;

/// The commands of the attached clients and of the request handlers that
/// wait for the task of one session. A sender waits when the queue is full,
/// thus input that the holder does not take uses no more memory.
const COMMAND_QUEUE: usize = 64;
const FALLBACK_SHELL: &str = "/bin/sh";

/// One session with its description, for a client.
pub(crate) struct ListedSession {
    pub(crate) record: SessionRecord,
    /// What the holder of the session wrote about it. `None` if the session
    /// has no metadata that the daemon can read.
    pub(crate) metadata: Option<SessionMetadata>,
    pub(crate) restore_policy: RestorePolicy,
    /// The working directory of the metadata with its symbolic links
    /// resolved. `None` if it cannot be resolved.
    pub(crate) resolved_working_directory: Option<PathBuf>,
}

/// The sessions of the host.
pub(crate) struct Sessions {
    runtime_directory: PathBuf,
    registry: Arc<SessionRegistry>,
    store: Arc<SessionStore>,
    stopped: Arc<StoppedSessions>,
    default_restore_policy: RestorePolicy,
    shutdown: Shutdown,
    status: Arc<StatusRegistry>,
    /// One restore at a time: two clients that open the same dormant
    /// session start one holder.
    restoring: Mutex<()>,
    /// The shell of the user: the command of a session without a command.
    shell: OsString,
    /// The home directory of the user: the working directory of a session
    /// without one, and of the shells of the pool.
    home: PathBuf,
    pool: HolderPool,
    resolved_directories: ResolvedDirectories,
}

impl Sessions {
    /// The sessions of the user of this daemon.
    ///
    /// # Errors
    ///
    /// Returns an error if the state directory or the home directory of the
    /// user has no path.
    pub(crate) fn new(
        runtime_directory: PathBuf,
        settings: Settings,
        shutdown: Shutdown,
        status: Arc<StatusRegistry>,
    ) -> io::Result<Self> {
        let registry: Arc<SessionRegistry> = Arc::default();
        let store = Arc::new(SessionStore::locate()?);
        let stopped = Arc::new(StoppedSessions::new(
            Arc::clone(&registry),
            Arc::clone(&store),
            Arc::clone(&status),
            settings.ended_session_retention,
        ));
        let shell = env::var_os("SHELL").unwrap_or_else(|| FALLBACK_SHELL.into());
        let home = env::var_os("HOME")
            .filter(|home| !home.is_empty())
            .map(PathBuf::from)
            .ok_or_else(|| io::Error::other("the HOME variable of the daemon is not set"))?;
        Ok(Self {
            runtime_directory,
            registry,
            store,
            stopped,
            default_restore_policy: settings.default_restore_policy,
            shutdown,
            status,
            restoring: Mutex::new(()),
            pool: HolderPool::new(shell.clone(), settings.pool_size, settings.pool_idle_time),
            shell,
            home,
            resolved_directories: ResolvedDirectories::default(),
        })
    }

    /// Starts a session with `command`: an empty command is the shell of the
    /// user, and no working directory is the home directory of the user. A
    /// session with the shell takes a holder of the pool if it has one. Else
    /// the daemon starts a holder and connects to it.
    pub(crate) async fn create(
        self: &Arc<Self>,
        command: Vec<String>,
        working_directory: Option<PathBuf>,
    ) -> Result<ListedSession, SessionError> {
        let working_directory = self.working_directory_of_session(working_directory)?;
        // The pool keeps its holders for the idle time from now, and fills
        // again after this session. Its task runs after the response: the
        // start of a holder does not delay this session.
        let created = self.create_with(command, working_directory).await;
        self.start_pool();
        created
    }

    async fn create_with(
        &self,
        command: Vec<String>,
        working_directory: PathBuf,
    ) -> Result<ListedSession, SessionError> {
        if command.is_empty()
            && let Some(pooled) = self.take_pooled(&working_directory).await
        {
            return Ok(self.describe(self.supervise(pooled.session_id, Some(pooled.link))));
        }
        let command: Vec<OsString> = if command.is_empty() {
            vec![self.shell.clone()]
        } else {
            command.into_iter().map(OsString::from).collect()
        };
        let session_id = self.unused_session_id().map_err(start_failure)?;
        let start = HolderStart {
            session_id: &session_id,
            command: &command,
            working_directory: &working_directory,
            size: None,
            pooled: false,
        };
        let link = self.start_holder(start).await.inspect_err(|_| {
            // A holder that did not start leaves the state directory of a
            // session that never ran.
            let _ = self.store.remove(&session_id);
        })?;
        Ok(self.describe(self.supervise(session_id, Some(link))))
    }

    /// The sessions in the sequence of their identifiers. The ended
    /// sessions are in the list only with `include_ended`.
    pub(crate) fn list(&self, include_ended: bool) -> Vec<ListedSession> {
        let records = self.registry.records();
        self.resolved_directories
            .keep_only(&records.iter().map(|record| &record.id).collect::<Vec<_>>());
        records
            .into_iter()
            .filter(|record| include_ended || !matches!(record.state, SessionState::Ended(_)))
            .map(|record| self.describe(record))
            .collect()
    }

    /// The session `id`, if the daemon has a connection to its holder. A
    /// request for the terminal of a session cannot wait for a holder that
    /// does not answer.
    pub(crate) fn find_reachable(&self, id: &str) -> Result<Arc<SessionHandle>, SessionError> {
        let (record, handle) = self.find(id)?;
        match (record.state, handle) {
            (SessionState::Running { .. }, Some(handle)) => Ok(handle),
            (SessionState::Dormant, _) => Err(SessionError::Failed(format!(
                "session {id} is dormant: its command does not run. Run `nomux start {id}`"
            ))),
            (SessionState::Damaged, _) => Err(damaged(&record)),
            _ => Err(SessionError::Failed(format!(
                "the daemon has no connection to the holder of session {id}"
            ))),
        }
    }

    /// Ends the session `id` and waits until its command has ended. A
    /// session without a holder leaves the list with its state directory.
    pub(crate) async fn kill(&self, id: &str) -> Result<(), SessionError> {
        if let Some(handle) = self.find(id)?.1 {
            return handle.kill().await;
        }
        // A restore that runs now completes first: the session then has a
        // holder, and its command gets the signal.
        let restore_completes = self.restoring.lock().await;
        let (record, handle) = self.find(id)?;
        drop(restore_completes);
        if let Some(handle) = handle {
            return handle.kill().await;
        }
        self.stopped.forget(&record.id);
        self.status.remove(&record.id);
        Ok(())
    }

    /// Detaches each client that is attached to the session `id`. Returns
    /// their number.
    pub(crate) fn detach(&self, id: &str) -> Result<usize, SessionError> {
        let (_, handle) = self.find(id)?;
        Ok(handle.map_or(0, |handle| handle.clients.detach_all()))
    }

    /// Sets when the daemon starts the command of the session `id` again.
    pub(crate) fn set_restore_policy(
        &self,
        id: &str,
        policy: RestorePolicy,
    ) -> Result<(), SessionError> {
        let (record, _) = self.find(id)?;
        if record.state == SessionState::Damaged {
            return Err(damaged(&record));
        }
        self.store
            .set_restore_policy(&record.id, policy)
            .map_err(|error| {
                SessionError::Failed(format!(
                    "the restore policy of session {id} cannot be stored: {error}"
                ))
            })
    }

    /// The output history of the session `id`, also of an ended session.
    pub(crate) async fn journal(&self, id: &str) -> Result<SessionJournal, SessionError> {
        let (record, handle) = self.find_with_ended(id)?;
        let directory = self.store.journal_directory(&record.id);
        // Without a connection to the holder, the history is what the
        // segment files have: it can lack the output of the last second.
        let feed = match (record.state, handle) {
            (SessionState::Running { .. }, Some(handle)) => handle.output.get().await.ok(),
            _ => None,
        };
        Ok(match feed {
            Some(feed) => self.live_journal(&record.id, feed),
            None => SessionJournal::stored(directory),
        })
    }

    /// The output history of a session whose holder answers on `feed`.
    pub(crate) fn live_journal(&self, id: &SessionId, feed: Arc<OutputFeed>) -> SessionJournal {
        SessionJournal::live(self.store.journal_directory(id), feed)
    }

    /// The restore policy of the session `id`: the one that a client gave
    /// to it, or the default of the configuration file.
    fn restore_policy(&self, id: &SessionId) -> RestorePolicy {
        self.store
            .restore_policy(id)
            .unwrap_or(self.default_restore_policy)
    }

    fn describe(&self, record: SessionRecord) -> ListedSession {
        let metadata = self.store.metadata(&record.id).ok();
        let resolved_working_directory = metadata.as_ref().and_then(|metadata| {
            self.resolved_directories
                .of(&record.id, &metadata.working_directory)
        });
        ListedSession {
            metadata,
            restore_policy: self.restore_policy(&record.id),
            resolved_working_directory,
            record,
        }
    }

    /// The session `id`, if it is not an ended session. An ended session
    /// has only its record and its output.
    fn find(&self, id: &str) -> Result<(SessionRecord, Option<Arc<SessionHandle>>), SessionError> {
        let found = self.find_with_ended(id)?;
        match found.0.state {
            SessionState::Ended(_) => Err(SessionError::NotFound(format!(
                "session {id} ended: its command does not run"
            ))),
            _ => Ok(found),
        }
    }

    fn find_with_ended(
        &self,
        id: &str,
    ) -> Result<(SessionRecord, Option<Arc<SessionHandle>>), SessionError> {
        // Text that is not a session identifier names no session.
        id.parse()
            .ok()
            .and_then(|session_id: SessionId| self.registry.find(&session_id))
            .ok_or_else(|| SessionError::NotFound(format!("no session has the identifier {id}")))
    }

    /// Starts a holder and takes its lease.
    async fn start_holder(&self, start: HolderStart<'_>) -> Result<HolderLink, SessionError> {
        let session_id = start.session_id;
        holder::launch(&self.runtime_directory, self.store.state_directory(), start)
            .await
            .map_err(|error| match error {
                LaunchError::NotReady => SessionError::Timeout(error.to_string()),
                _ => start_failure(error),
            })?;
        holder::open_with_lease(&self.holder_socket(session_id), session_id)
            .await
            .map_err(start_failure)
    }

    /// Puts a session into the list and starts the task that owns the
    /// connection to its holder and its status task. Without a link, the
    /// task connects itself.
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
        tokio::spawn(status::watch(
            session_id.clone(),
            self.holder_socket(&session_id),
            ended.clone(),
            Arc::clone(&self.status),
            Arc::clone(&self.registry),
        ));
        let handle = SessionHandle::new(session_id.clone(), commands, ended, output);
        let record = SessionRecord {
            id: session_id.clone(),
            state,
            damage: None,
        };
        self.registry.put(record.clone(), Some(Arc::new(handle)));
        let supervisor = Supervisor::new(
            session_id,
            self.runtime_directory.clone(),
            Arc::clone(&self.registry),
            Arc::clone(&self.stopped),
            self.shutdown.clone(),
            command_receiver,
            ended_sender,
        );
        tokio::spawn(supervisor.run(link));
        record
    }

    fn working_directory_of_session(
        &self,
        requested: Option<PathBuf>,
    ) -> Result<PathBuf, SessionError> {
        let directory = requested.unwrap_or_else(|| self.home.clone());
        if !is_absolute_directory(&directory) {
            return Err(SessionError::Failed(format!(
                "the working directory {} is not an absolute path of a directory",
                directory.display()
            )));
        }
        Ok(directory)
    }

    fn holder_socket(&self, session_id: &SessionId) -> PathBuf {
        holder_directory(&self.runtime_directory, session_id).join(SOCKET_FILE)
    }

    /// A random identifier that no session, no holder directory, no pooled
    /// holder and no state directory has.
    fn unused_session_id(&self) -> io::Result<SessionId> {
        loop {
            let session_id = session_id::random()?;
            let in_use = self.registry.contains(&session_id)
                || holder_directory(&self.runtime_directory, &session_id).exists()
                || pooled_holder_directory(&self.runtime_directory, &session_id).exists()
                || self.store.has(&session_id);
            if !in_use {
                return Ok(session_id);
            }
        }
    }
}

fn is_absolute_directory(path: &Path) -> bool {
    path.is_absolute() && path.is_dir()
}

fn start_failure(error: impl fmt::Display) -> SessionError {
    SessionError::Failed(format!("the session did not start: {error}"))
}

fn damaged(record: &SessionRecord) -> SessionError {
    SessionError::Failed(format!(
        "session {} is damaged: {}",
        record.id,
        record
            .damage
            .as_deref()
            .unwrap_or("its state cannot be read")
    ))
}
