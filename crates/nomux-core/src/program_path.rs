//! Finds a program in the directories of `PATH`.

use std::env;
use std::path::PathBuf;

/// The path of the program `name`: `name` itself if it has a `/`, else the
/// first file `name` in a directory of `PATH`. `None` if there is no such
/// file.
#[must_use]
pub fn program_in_path(name: &str) -> Option<PathBuf> {
    if name.contains('/') {
        return Some(PathBuf::from(name)).filter(|path| path.is_file());
    }
    env::split_paths(&env::var_os("PATH")?)
        .map(|directory| directory.join(name))
        .find(|path| path.is_file())
}
