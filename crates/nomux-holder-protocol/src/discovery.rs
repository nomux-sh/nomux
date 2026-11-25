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
//! The start time in `pid` tells a holder from a later process that got the
//! same process ID. [`scan`] reads the directories and gives the state of
//! each holder. The daemon needs no other source to find the holders.

mod process_identity;
mod process_record;
mod scan;

use std::env;
use std::path::{Path, PathBuf};

pub use process_identity::ProcessIdentity;
pub use process_record::program_name;
pub use scan::{DiscoveredHolder, HolderState, StaleReason, scan};

use crate::SessionId;

/// The name of the file with the process identity of the holder.
pub const PID_FILE: &str = "pid";
/// The name of the control socket.
pub const SOCKET_FILE: &str = "sock";
/// The name of the file that the holder writes last.
pub const READY_FILE: &str = "ready";

/// The runtime directory of the user: `~/.nomux/run`. Returns `None` if the
/// `HOME` variable is not set.
#[must_use]
pub fn default_runtime_directory() -> Option<PathBuf> {
    let home = env::var_os("HOME").filter(|home| !home.is_empty())?;
    Some(PathBuf::from(home).join(".nomux").join("run"))
}

/// The directory of the holder of one session.
#[must_use]
pub fn holder_directory(runtime_directory: &Path, session_id: &SessionId) -> PathBuf {
    runtime_directory.join(session_id.as_str())
}
