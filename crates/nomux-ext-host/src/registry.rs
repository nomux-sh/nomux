//! The installed extensions: their packages, their state and the
//! permissions that the user approved.

mod grant;
mod install_error;
mod layout;
mod package;
mod pending_install;
mod removed_extension;
mod state_file;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use nomux_extension::Permissions;
use nomux_extension::manifest::Manifest;
use tracing::warn;

pub use install_error::{InstallError, StateError};
use layout::{Layout, package_file};
pub(crate) use pending_install::PendingInstall;
pub(crate) use removed_extension::RemovedExtension;

/// Whether the host starts an extension.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Enablement {
    /// The host starts the extension when a call needs it.
    Enabled,
    /// The host does not start the extension.
    Disabled {
        /// Why the extension is disabled: the user disabled it, or it
        /// failed too many times.
        reason: String,
    },
}

/// One installed extension.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct InstalledExtension {
    /// The manifest of the package.
    pub manifest: Manifest,
    /// The permissions that the user approved.
    pub granted: Permissions,
    /// Whether the host starts the extension.
    pub enablement: Enablement,
}

/// The installed extensions of one host directory. The registry starts no
/// process and has no task.
#[derive(Debug)]
pub(crate) struct Registry {
    layout: Layout,
    extensions: BTreeMap<String, InstalledExtension>,
    /// The identifiers of the packages that an installation copies now.
    installing: BTreeSet<String>,
}

impl Registry {
    /// Reads the installed extensions. A package that is not valid is not in
    /// the registry: one bad package must not stop the daemon. A package
    /// that the host cannot run stays in the registry, and a call says why.
    pub(crate) fn load(directory: PathBuf) -> io::Result<Self> {
        let layout = Layout::new(directory);
        let mut extensions = BTreeMap::new();
        for package in package_directories(&layout.packages())? {
            match load_extension(&layout, &package) {
                Ok(extension) => {
                    extensions.insert(extension.manifest.id.clone(), extension);
                }
                Err(error) => {
                    warn!(package = %package.display(), %error, "the package is not valid; the host does not use it");
                }
            }
        }
        Ok(Self {
            layout,
            extensions,
            installing: BTreeSet::new(),
        })
    }

    /// Accepts the package in `source` for an installation with the
    /// permissions that the user approved. Copies nothing.
    pub(crate) fn begin_install(
        &mut self,
        source: &Path,
        granted: Permissions,
    ) -> Result<PendingInstall, InstallError> {
        let manifest = inspect(source)?;
        let id = manifest.id.clone();
        if self.extensions.contains_key(&id) {
            return Err(InstallError::AlreadyInstalled { id });
        }
        if let Some(permission) = grant::first_not_requested(&manifest.permissions, &granted) {
            return Err(InstallError::NotRequested { permission });
        }
        if !self.installing.insert(id.clone()) {
            return Err(InstallError::InProgress { id });
        }
        Ok(PendingInstall {
            incoming: self.layout.incoming_package(&id),
            source: source.to_path_buf(),
            manifest,
            granted,
        })
    }

    /// Completes an installation after the copy of its package: records the
    /// approved permissions and enables the extension. If the copy failed,
    /// removes what it left.
    pub(crate) fn finish_install(
        &mut self,
        pending: PendingInstall,
        copied: Result<(), InstallError>,
    ) -> Result<InstalledExtension, InstallError> {
        let id = pending.manifest.id.clone();
        self.installing.remove(&id);
        let state_file = self.layout.state_file(&id);
        let placed = copied.and_then(|()| {
            // The state file comes first, thus each package has its state
            // file.
            state_file::write(&state_file, &Enablement::Enabled, &pending.granted)?;
            fs::rename(&pending.incoming, self.layout.package(&id))?;
            Ok(())
        });
        if let Err(error) = placed {
            // The reason of the installation failure is the result. A file
            // that stays here is replaced by the next installation.
            let _ = fs::remove_file(&state_file);
            let _ = fs::remove_dir_all(&pending.incoming);
            return Err(error);
        }

        let extension = InstalledExtension {
            manifest: pending.manifest,
            granted: pending.granted,
            enablement: Enablement::Enabled,
        };
        self.extensions.insert(id, extension.clone());
        Ok(extension)
    }

    /// Takes an extension out of the registry: removes its state file and
    /// gives its package and its data names that the registry does not
    /// read. The result deletes these directories.
    pub(crate) fn remove(&mut self, id: &str) -> Result<RemovedExtension, StateError> {
        if !self.extensions.contains_key(id) {
            return Err(StateError::NotInstalled);
        }
        let removed = RemovedExtension {
            package: self.layout.removed_package(id),
            data: self.layout.removed_data(id),
        };
        // Directories that an earlier removal did not delete.
        removed.delete_files().map_err(StateError::Io)?;
        removed
            .take_package(&self.layout.package(id))
            .map_err(StateError::Io)?;
        // Without its package, the extension is not installed.
        self.extensions.remove(id);
        removed
            .take_state_and_data(&self.layout.state_file(id), &self.layout.data(id))
            .map_err(StateError::Io)?;
        Ok(removed)
    }

    /// Changes whether the host starts an extension, in memory and in the
    /// state file.
    pub(crate) fn set_enablement(
        &mut self,
        id: &str,
        enablement: Enablement,
    ) -> Result<(), StateError> {
        let extension = self
            .extensions
            .get_mut(id)
            .ok_or(StateError::NotInstalled)?;
        // The new state applies also when the file is not written: the host
        // must not start an extension that it disabled.
        extension.enablement = enablement;
        state_file::write(
            &self.layout.state_file(id),
            &extension.enablement,
            &extension.granted,
        )
        .map_err(StateError::Io)
    }

    pub(crate) fn get(&self, id: &str) -> Option<&InstalledExtension> {
        self.extensions.get(id)
    }

    pub(crate) fn extensions(&self) -> impl Iterator<Item = &InstalledExtension> {
        self.extensions.values()
    }

    /// The program of the daemon part. `None` if the package has no daemon
    /// part.
    pub(crate) fn program(&self, extension: &InstalledExtension) -> Option<PathBuf> {
        let daemon = extension.manifest.daemon.as_ref()?;
        let package = self.layout.package(&extension.manifest.id);
        Some(package_file(&package, &daemon.command))
    }

    /// The working directory of the process of an extension.
    pub(crate) fn data_directory(&self, id: &str) -> PathBuf {
        self.layout.data(id)
    }
}

/// Reads the manifest of the package in `source` and examines if the host
/// can run the package.
pub(crate) fn inspect(source: &Path) -> Result<Manifest, InstallError> {
    let manifest = package::read_manifest(source)?;
    package::check_runnable(source, &manifest)?;
    Ok(manifest)
}

/// The directories in `packages` that can be a package. The result is empty
/// if no extension was installed yet.
fn package_directories(packages: &Path) -> io::Result<Vec<PathBuf>> {
    let entries = match fs::read_dir(packages) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut directories = Vec::new();
    for entry in entries {
        let entry = entry?;
        let is_temporary = entry.file_name().to_string_lossy().starts_with('.');
        if entry.file_type()?.is_dir() && !is_temporary {
            directories.push(entry.path());
        }
    }
    Ok(directories)
}

fn load_extension(layout: &Layout, package: &Path) -> Result<InstalledExtension, InstallError> {
    let manifest = package::read_manifest(package)?;
    if package.file_name() != Some(manifest.id.as_ref()) {
        return Err(InstallError::DirectoryName { id: manifest.id });
    }
    let (enablement, granted) = state_file::read(&layout.state_file(&manifest.id))?;
    Ok(InstalledExtension {
        manifest,
        granted,
        enablement,
    })
}
