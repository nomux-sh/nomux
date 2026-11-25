//! Finds a nomux binary that is installed together with the binary that runs.

use std::env;
use std::path::PathBuf;

/// The path of the nomux binary `name`.
///
/// A release puts `nomux`, `nomuxd` and `nomux-holder` into one directory.
/// Thus the function gives the file `name` in the directory of the binary
/// that runs. If there is no such file, it gives `name` without a directory,
/// and the operating system looks for it in `PATH`.
#[must_use]
pub fn companion_binary(name: &str) -> PathBuf {
    let sibling = env::current_exe()
        .ok()
        .and_then(|executable| Some(executable.parent()?.join(name)));
    match sibling {
        Some(path) if path.is_file() => path,
        _ => PathBuf::from(name),
    }
}
