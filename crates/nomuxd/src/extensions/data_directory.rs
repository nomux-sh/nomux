//! The directory of the extension host: the packages of the installed
//! extensions, their state files and their working directories.

use std::env;
use std::fs::{self, DirBuilder, Permissions};
use std::io;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};

const OWNER_ONLY: u32 = 0o700;

#[cfg(target_os = "macos")]
const HOME_DATA_DIRECTORY: &str = "Library/Application Support";
#[cfg(not(target_os = "macos"))]
const HOME_DATA_DIRECTORY: &str = ".local/share";

/// The directory of the extensions of the user.
///
/// | Host | Directory |
/// |---|---|
/// | Linux | `~/.local/share/nomux/extensions` |
/// | macOS | `~/Library/Application Support/nomux/extensions` |
///
/// The path comes from `HOME` only, as the path of the runtime directory
/// does.
pub(super) fn locate() -> io::Result<PathBuf> {
    let home = env::var_os("HOME")
        .filter(|home| !home.is_empty())
        .ok_or_else(|| {
            io::Error::other(
                "the HOME variable is not set, thus the directory of the extensions has no path",
            )
        })?;
    Ok(PathBuf::from(home)
        .join(HOME_DATA_DIRECTORY)
        .join("nomux")
        .join("extensions"))
}

/// Makes the directory if it does not exist, and makes it private to the
/// user: an approval of permissions must not be open to other users.
pub(super) fn prepare(path: &Path) -> io::Result<()> {
    DirBuilder::new()
        .recursive(true)
        .mode(OWNER_ONLY)
        .create(path)?;
    fs::set_permissions(path, Permissions::from_mode(OWNER_ONLY))
}
