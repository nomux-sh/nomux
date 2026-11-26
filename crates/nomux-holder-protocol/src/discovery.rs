//! The discovery files of a holder.
//!
//! Each holder has one directory in the runtime directory of the user. The
//! name of the directory is the session identifier.
//!
//! ```text
//! ~/.nomux/run/            runtime directory, mode 0700
//!   <session identifier>/  holder directory, mode 0700
//!     pid                  "<process ID> <start time>\n"
//!     sock                 control socket, mode 0600
//!     ready                empty; the holder writes it last
//! ```
//!
//! A pooled holder has no session yet. Its directory has the name
//! `<session identifier>.pool`, which is not a session identifier: the
//! identifier is the one that the daemon keeps for the session that the
//! holder becomes. When the holder gets its session, it renames the
//! directory to the session identifier.
//!
//! The start time in `pid` tells a holder from a later process that got the
//! same process ID. [`scan`] reads the directories of the session holders
//! and [`scan_pooled`] the directories of the pooled holders, and they give
//! the state of each holder. The daemon needs no other source to find the
//! holders.

mod process_identity;
mod process_record;
mod scan;

use std::path::{Path, PathBuf};

pub use process_identity::ProcessIdentity;
pub use process_record::{program_name, terminal_foreground_group};
pub use scan::{DiscoveredHolder, HolderState, StaleReason, scan, scan_pooled};

use crate::SessionId;

/// The name of the file with the process identity of the holder.
pub const PID_FILE: &str = "pid";
/// The name of the control socket.
pub const SOCKET_FILE: &str = "sock";
/// The name of the file that the holder writes last.
pub const READY_FILE: &str = "ready";
/// The end of the name of the directory of a pooled holder.
pub const POOLED_DIRECTORY_SUFFIX: &str = ".pool";
/// The name of the socket of the daemon in the runtime directory. A holder
/// gives its path to the command of its session.
pub const DAEMON_SOCKET_FILE: &str = "nomuxd.sock";
/// The directory of the agent socket of the daemon in the runtime
/// directory: the socket that a sandboxed session can use. The name has a
/// `.`, thus it is not a session identifier. A sandbox permits the
/// directory, and not the socket file: the directory stays when the daemon
/// starts again, and the socket file is new.
pub const AGENT_SOCKET_DIRECTORY: &str = "nomuxd.agent";
/// The name of the agent socket in its directory.
pub const AGENT_SOCKET_FILE: &str = "sock";

/// The directory of the holder of one session.
#[must_use]
pub fn holder_directory(runtime_directory: &Path, session_id: &SessionId) -> PathBuf {
    runtime_directory.join(session_id.as_str())
}

/// The directory of a pooled holder that keeps the identifier of one
/// session.
#[must_use]
pub fn pooled_holder_directory(runtime_directory: &Path, session_id: &SessionId) -> PathBuf {
    runtime_directory.join(format!("{session_id}{POOLED_DIRECTORY_SUFFIX}"))
}
