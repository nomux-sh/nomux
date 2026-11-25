//! Where the global configuration file is.

use std::env;
use std::path::PathBuf;

const DIRECTORY: &str = "nomux";
const FILE: &str = "config.toml";

/// The path of the global configuration file of this user.
///
/// - If `XDG_CONFIG_HOME` is an absolute path: `$XDG_CONFIG_HOME/nomux/config.toml`.
/// - On macOS: `~/Library/Application Support/nomux/config.toml`.
/// - On the other hosts: `~/.config/nomux/config.toml`.
///
/// Returns `None` if the `HOME` variable is necessary and is not set.
#[must_use]
pub fn default_configuration_path() -> Option<PathBuf> {
    let from_environment = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute());
    let base = if let Some(base) = from_environment {
        base
    } else {
        let home = env::var_os("HOME").filter(|home| !home.is_empty())?;
        PathBuf::from(home).join(PLATFORM_DIRECTORY)
    };
    Some(base.join(DIRECTORY).join(FILE))
}

#[cfg(target_os = "macos")]
const PLATFORM_DIRECTORY: &str = "Library/Application Support";

#[cfg(not(target_os = "macos"))]
const PLATFORM_DIRECTORY: &str = ".config";
