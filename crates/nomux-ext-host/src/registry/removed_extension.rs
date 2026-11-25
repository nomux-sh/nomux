//! The files of an extension that the registry removed.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// The package and the data of an extension that is not in the registry
/// now. The two directories have names that the registry does not read.
///
/// The deletion of the directories takes time, thus it runs without the
/// lock of the registry: `Registry::remove`, then
/// [`RemovedExtension::delete_files`].
#[derive(Debug)]
pub(crate) struct RemovedExtension {
    pub(super) package: PathBuf,
    pub(super) data: PathBuf,
}

impl RemovedExtension {
    /// Deletes the package and the data.
    pub(crate) fn delete_files(&self) -> io::Result<()> {
        ignore_not_found(fs::remove_dir_all(&self.package))?;
        ignore_not_found(fs::remove_dir_all(&self.data))
    }

    /// Moves the installed package away. After this, a new host does not
    /// find the extension.
    pub(super) fn take_package(&self, installed: &Path) -> io::Result<()> {
        ignore_not_found(fs::rename(installed, &self.package))
    }

    /// Removes the state file and moves the data away.
    pub(super) fn take_state_and_data(&self, state_file: &Path, data: &Path) -> io::Result<()> {
        ignore_not_found(fs::remove_file(state_file))?;
        ignore_not_found(fs::rename(data, &self.data))
    }
}

/// An extension that never ran has no data directory.
fn ignore_not_found(result: io::Result<()>) -> io::Result<()> {
    match result {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}
