//! The session state directory: what a session keeps on disk.
//!
//! Each session has one directory in the state directory of the user. The
//! name of the directory is the session identifier.
//!
//! ```text
//! <state directory>/         mode 0700
//!   sessions/                mode 0700
//!     <session identifier>/  mode 0700
//!       session              the session metadata, mode 0600
//!       journal/             mode 0700
//!         <epoch>-<start position>.journal  one segment, mode 0600
//! ```
//!
//! The state directory is not the runtime directory of the
//! [discovery files](crate::discovery): its contents must stay when the
//! host starts again. The holder writes the directory and never removes it.
//! The daemon reads it, also when the session does not run, and removes it
//! when it forgets the session.
//!
//! - [`SessionMetadata`] is the `session` file.
//! - [`journal`](crate::journal) is the format of the `journal` directory.

mod metadata;
mod metadata_text;

use std::env;
use std::path::{Path, PathBuf};

pub use metadata::{InvalidMetadata, RestorePolicy, SessionMetadata};

use crate::SessionId;

/// The name of the file with the session metadata.
pub const METADATA_FILE: &str = "session";
/// The name of the directory with the journal.
pub const JOURNAL_DIRECTORY: &str = "journal";

const SESSIONS_DIRECTORY: &str = "sessions";

/// The state directory of the user. Returns `None` if the `HOME` variable
/// is not set.
///
/// | Host | Directory |
/// |---|---|
/// | Linux | `~/.local/state/nomux` |
/// | macOS | `~/Library/Application Support/nomux` |
#[must_use]
pub fn default_state_directory() -> Option<PathBuf> {
    let home = env::var_os("HOME").filter(|home| !home.is_empty())?;
    Some(PathBuf::from(home).join(HOME_STATE_DIRECTORY).join("nomux"))
}

#[cfg(target_os = "macos")]
const HOME_STATE_DIRECTORY: &str = "Library/Application Support";
#[cfg(not(target_os = "macos"))]
const HOME_STATE_DIRECTORY: &str = ".local/state";

/// The directory of one session in `state_directory`.
#[must_use]
pub fn session_state_directory(state_directory: &Path, session_id: &SessionId) -> PathBuf {
    state_directory
        .join(SESSIONS_DIRECTORY)
        .join(session_id.as_str())
}
