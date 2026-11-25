//! The installed extensions: their packages, their state and the
//! permissions that the user approved.

mod grant;
mod install_error;
mod layout;
mod package;
mod state_file;

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use nomux_extension::Permissions;
use nomux_extension::manifest::Manifest;
use tracing::warn;

pub use install_error::{InstallError, StateError};
use layout::{Layout, package_file};

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
        Ok(Self { layout, extensions })
    }

    /// Copies the package in `source` to the host directory and records the
    /// permissions that the user approved. The extension is enabled.
    pub(crate) fn install(
        &mut self,
        source: &Path,
        granted: Permissions,
    ) -> Result<InstalledExtension, InstallError> {
        let manifest = package::read_manifest(source)?;
        package::check_runnable(source, &manifest)?;
        let id = manifest.id.clone();
        if self.extensions.contains_key(&id) {
            return Err(InstallError::AlreadyInstalled { id });
        }
        if let Some(permission) = grant::first_not_requested(&manifest.permissions, &granted) {
            return Err(InstallError::NotRequested { permission });
        }

        let incoming = self.layout.incoming_package(&id);
        if incoming.exists() {
            // An earlier installation stopped in the middle of the copy.
            fs::remove_dir_all(&incoming)?;
        }
        fs::create_dir_all(self.layout.packages())?;
        package::copy(source, &incoming)?;
        // The state file comes first, thus each package has its state file.
        state_file::write(&self.layout.state_file(&id), &Enablement::Enabled, &granted)?;
        fs::rename(incoming, self.layout.package(&id))?;

        let extension = InstalledExtension {
            manifest,
            granted,
            enablement: Enablement::Enabled,
        };
        self.extensions.insert(id, extension.clone());
        Ok(extension)
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
        let is_incoming = entry.file_name().to_string_lossy().starts_with('.');
        if entry.file_type()?.is_dir() && !is_incoming {
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
