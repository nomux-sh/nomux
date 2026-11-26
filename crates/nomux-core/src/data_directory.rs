//! Where nomux keeps the data of this user that stays after a restart of
//! the host: the installed extensions and the records of the hook installs.

use std::env;
use std::path::PathBuf;

#[cfg(target_os = "macos")]
const HOME_DATA_DIRECTORY: &str = "Library/Application Support";
#[cfg(not(target_os = "macos"))]
const HOME_DATA_DIRECTORY: &str = ".local/share";

/// The data directory of nomux for this user.
///
/// | Host | Directory |
/// |---|---|
/// | Linux | `~/.local/share/nomux` |
/// | macOS | `~/Library/Application Support/nomux` |
///
/// The path comes from `HOME` only, as the path of the runtime directory
/// does. Returns `None` if `HOME` is not set.
#[must_use]
pub fn data_directory() -> Option<PathBuf> {
    let home = env::var_os("HOME").filter(|home| !home.is_empty())?;
    Some(PathBuf::from(home).join(HOME_DATA_DIRECTORY).join("nomux"))
}
