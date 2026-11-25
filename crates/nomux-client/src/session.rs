//! The sessions of a host, as a client sees them.

use std::path::PathBuf;

use nomux_proto::wire::v1 as wire;

/// The state of a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    /// The command of the session runs.
    Running,
    /// The holder of the session runs, but the daemon has no connection to
    /// it. The daemon tries to connect again.
    Unreachable,
    /// A state from a later protocol version.
    Unknown,
}

/// One session on a host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    /// The session identifier.
    pub id: String,
    /// The state of the session.
    pub state: SessionState,
    /// The process ID of the command on the host, if the daemon knows it.
    pub pid: Option<u32>,
}

impl From<wire::Session> for Session {
    fn from(session: wire::Session) -> Self {
        let state = match wire::SessionState::try_from(session.state) {
            Ok(wire::SessionState::Running) => SessionState::Running,
            Ok(wire::SessionState::Unreachable) => SessionState::Unreachable,
            Ok(wire::SessionState::Unspecified) | Err(_) => SessionState::Unknown,
        };
        Self {
            id: session.id,
            state,
            pid: (session.pid != 0).then_some(session.pid),
        }
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
}
