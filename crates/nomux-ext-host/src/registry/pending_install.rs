//! An installation between its acceptance and its completion.

use std::fs;
use std::path::PathBuf;

use nomux_extension::Permissions;
use nomux_extension::manifest::Manifest;

use super::install_error::InstallError;
use super::package;

/// An installation that the registry accepted and that has no package in
/// the host directory yet.
///
/// The copy of the package takes time, thus it runs without the lock of
/// the registry: `Registry::begin_install`, then
/// [`PendingInstall::copy_package`], then `Registry::finish_install`.
#[derive(Debug)]
pub(crate) struct PendingInstall {
    pub(super) manifest: Manifest,
    pub(super) granted: Permissions,
    pub(super) source: PathBuf,
    /// Where the package is until the installation is complete.
    pub(super) incoming: PathBuf,
}

impl PendingInstall {
    /// Copies the package to its place for incoming packages.
    pub(crate) fn copy_package(&self) -> Result<(), InstallError> {
        if self.incoming.exists() {
            // An earlier installation stopped in the middle of the copy.
            fs::remove_dir_all(&self.incoming)?;
        }
        if let Some(packages) = self.incoming.parent() {
            fs::create_dir_all(packages)?;
        }
        package::copy(&self.source, &self.incoming)
    }
}
