//! The sandbox profile of one session on disk: the file `sandbox` in its
//! state directory, with one line: the name of the profile. A session
//! without a sandbox has no file.
//!
//! The daemon owns this file. A restore and a wake read it, thus a session
//! keeps its profile after a restart of the daemon and of the host.

use std::io;
use std::path::Path;

use nomux_core::configuration::is_profile_name;

use super::small_file;

const FILE: &str = "sandbox";
/// A profile name and its line break.
const MAX_LENGTH: u64 = 65;

/// The profile in `session_directory`. `None` if the session has none.
///
/// # Errors
///
/// Returns the reason if the file exists and cannot be read or has no valid
/// name: a session whose profile is not known must not start without it.
pub(super) fn read(session_directory: &Path) -> Result<Option<String>, String> {
    let bytes = match small_file::read(&session_directory.join(FILE), MAX_LENGTH) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("the sandbox file cannot be read: {error}")),
    };
    std::str::from_utf8(&bytes)
        .ok()
        .map(str::trim_end)
        .filter(|name| is_profile_name(name))
        .map(|name| Some(name.to_owned()))
        .ok_or_else(|| "the sandbox file has no valid profile name".to_owned())
}

/// Writes `profile` to `session_directory`.
pub(super) fn write(session_directory: &Path, profile: &str) -> io::Result<()> {
    small_file::replace(session_directory, FILE, format!("{profile}\n").as_bytes())
}
