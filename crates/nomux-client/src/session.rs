//! The sessions of a host, as a client sees them.

use std::path::PathBuf;

use nomux_core::configuration::RestorePolicy;
use nomux_proto::wire::v1 as wire;

use crate::{Hibernation, WindowSize};

/// The state of a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    /// The command of the session runs.
    Running,
    /// The holder of the session runs, but the daemon has no connection to
    /// it. The daemon tries to connect again.
    Unreachable,
    /// The session has no process: its holder is gone, and its command did
    /// not end. This is the state after a restart of the host. The daemon
    /// has the description and the output of the session, and it can start
    /// the command again.
    Dormant,
    /// The command of the session ended. The daemon keeps the session and
    /// its output for a retention time.
    Ended,
    /// The daemon cannot read the description of the session.
    /// [`Session::damage`] has the reason.
    Damaged,
    /// A state from a later protocol version.
    Unknown,
}

/// How the command of a session ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitStatus {
    /// The command ended with an exit code.
    Exited {
        /// The exit code.
        code: i32,
    },
    /// A signal ended the command.
    Signaled {
        /// The signal number of the operating system of the host.
        signal: i32,
    },
    /// The daemon does not know how the command ended.
    Unknown,
}

/// How and when the command of a session ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionExit {
    /// When the command ended: seconds after the Unix epoch.
    pub ended_at: u64,
    /// How the command ended.
    pub status: ExitStatus,
}

/// One session on a host.
///
/// The daemon gives the description of a session also when the session has
/// no process. A field that the daemon does not know is `None` or empty.
/// The description never has the environment of the command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    /// The session identifier.
    pub id: String,
    /// The session name. `None` if the session has no name, and from a
    /// daemon without session names.
    pub name: Option<String>,
    /// The state of the session.
    pub state: SessionState,
    /// The process ID of the command on the host. `None` if the session has
    /// no process or the daemon does not know it.
    pub pid: Option<u32>,
    /// The program of the command and its arguments.
    pub command: Vec<String>,
    /// The working directory of the command, as the client of
    /// [`Client::create_session`](crate::Client::create_session) gave it.
    pub working_directory: Option<PathBuf>,
    /// The working directory with each symbolic link resolved on the host.
    /// `None` if the daemon cannot resolve it, and from an older daemon.
    pub resolved_working_directory: Option<PathBuf>,
    /// The size of the window of the session.
    pub size: Option<WindowSize>,
    /// When the command started in the current epoch: seconds after the
    /// Unix epoch.
    pub started_at: Option<u64>,
    /// The current epoch of the session, or its last epoch if the session
    /// has no process.
    pub epoch: Option<u32>,
    /// When the daemon starts the command again after the session became
    /// dormant. `None` for a policy from a later protocol version.
    pub restore_policy: Option<RestorePolicy>,
    /// How the command ended. Only for [`SessionState::Ended`].
    pub exit: Option<SessionExit>,
    /// Why the daemon cannot read a part of the state files of the session,
    /// in English for a person. With a state that is not
    /// [`SessionState::Damaged`], a part of the output is gone and the
    /// session can be used.
    pub damage: Option<String>,
    /// When a client attached to the session the last time: seconds after
    /// the Unix epoch. `None` if no client attached since the start of the
    /// daemon, and from an older daemon.
    pub last_attached_at: Option<u64>,
    /// The number of clients that are attached to the session now: 0 from
    /// an older daemon.
    pub attached_clients: u32,
    /// The last hibernation: while the session is hibernated, and while
    /// its command is the resumed run. `None` if the session never
    /// hibernated, and from an older daemon.
    pub hibernation: Option<Hibernation>,
    /// The user excluded the session from automatic hibernation.
    pub pinned: bool,
}

impl Session {
    /// The name of the session, or its identifier if it has no name.
    #[must_use]
    pub fn label(&self) -> &str {
        self.name.as_deref().unwrap_or(&self.id)
    }

    /// `true` if the daemon stopped the command of the agent of the session
    /// and waits for its wake.
    #[must_use]
    pub fn is_hibernated(&self) -> bool {
        self.state == SessionState::Dormant
            && self
                .hibernation
                .as_ref()
                .is_some_and(|hibernation| hibernation.woken_at.is_none())
    }
}

impl From<wire::Session> for Session {
    fn from(session: wire::Session) -> Self {
        let state = match wire::SessionState::try_from(session.state) {
            Ok(wire::SessionState::Running) => SessionState::Running,
            Ok(wire::SessionState::Unreachable) => SessionState::Unreachable,
            Ok(wire::SessionState::Dormant) => SessionState::Dormant,
            Ok(wire::SessionState::Ended) => SessionState::Ended,
            Ok(wire::SessionState::Damaged) => SessionState::Damaged,
            Ok(wire::SessionState::Unspecified) | Err(_) => SessionState::Unknown,
        };
        let size = match (u16::try_from(session.rows), u16::try_from(session.columns)) {
            (Ok(rows @ 1..), Ok(columns @ 1..)) => Some(WindowSize { rows, columns }),
            _ => None,
        };
        Self {
            id: session.id,
            name: (!session.name.is_empty()).then_some(session.name),
            state,
            pid: (session.pid != 0).then_some(session.pid),
            command: session.command,
            working_directory: (!session.working_directory.is_empty())
                .then(|| PathBuf::from(session.working_directory)),
            resolved_working_directory: (!session.resolved_working_directory.is_empty())
                .then(|| PathBuf::from(session.resolved_working_directory)),
            size,
            started_at: (session.started_at != 0).then_some(session.started_at),
            epoch: (session.epoch != 0).then_some(session.epoch),
            restore_policy: wire::RestorePolicy::try_from(session.restore_policy)
                .ok()
                .and_then(restore_policy_of),
            exit: session.exit.map(SessionExit::from),
            damage: (!session.damage.is_empty()).then_some(session.damage),
            last_attached_at: (session.last_attached_at != 0).then_some(session.last_attached_at),
            attached_clients: session.attached_clients,
            hibernation: session.hibernation.map(Hibernation::from),
            pinned: session.pinned,
        }
    }
}

impl From<wire::SessionExit> for SessionExit {
    fn from(exit: wire::SessionExit) -> Self {
        let status = match exit.status {
            Some(wire::session_exit::Status::ExitCode(code)) => ExitStatus::Exited { code },
            Some(wire::session_exit::Status::Signal(signal)) => ExitStatus::Signaled { signal },
            None => ExitStatus::Unknown,
        };
        Self {
            ended_at: exit.ended_at,
            status,
        }
    }
}

fn restore_policy_of(policy: wire::RestorePolicy) -> Option<RestorePolicy> {
    match policy {
        wire::RestorePolicy::Auto => Some(RestorePolicy::Auto),
        wire::RestorePolicy::OnFocus => Some(RestorePolicy::OnFocus),
        wire::RestorePolicy::Manual => Some(RestorePolicy::Manual),
        wire::RestorePolicy::Unspecified => None,
    }
}

pub(crate) fn wire_restore_policy(policy: RestorePolicy) -> wire::RestorePolicy {
    match policy {
        RestorePolicy::Auto => wire::RestorePolicy::Auto,
        RestorePolicy::OnFocus => wire::RestorePolicy::OnFocus,
        RestorePolicy::Manual => wire::RestorePolicy::Manual,
    }
}

/// The description of a session to start.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NewSession {
    /// The program and its arguments. If the list is empty, the daemon
    /// starts the shell of the user.
    pub command: Vec<String>,
    /// The working directory of the command: an absolute path on the host.
    /// `None` is the home directory of the user.
    pub working_directory: Option<PathBuf>,
    /// The name of the session. `None` for a session without a name.
    pub name: Option<String>,
    /// If a session that is not ended has `name`, the result is that
    /// session and no session starts.
    pub reuse_existing: bool,
}
