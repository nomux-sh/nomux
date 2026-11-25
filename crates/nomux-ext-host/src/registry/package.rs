//! One package directory: its manifest and its files.

use std::fs;
use std::path::Path;

use nomux_extension::ApiVersion;
use nomux_extension::manifest::Manifest;

use super::install_error::InstallError;
use super::layout::package_file;

const MANIFEST_FILE: &str = "extension.toml";

/// Reads and validates `extension.toml` of a package.
pub(super) fn read_manifest(package: &Path) -> Result<Manifest, InstallError> {
    let text = fs::read_to_string(package.join(MANIFEST_FILE))?;
    text.parse().map_err(InstallError::Manifest)
}

/// Examines what the host needs to run a package: the extension API version
/// and the program of the daemon part.
pub(super) fn check_runnable(package: &Path, manifest: &Manifest) -> Result<(), InstallError> {
    if !manifest.api.matches(ApiVersion::CURRENT) {
        return Err(InstallError::IncompatibleApi {
            required: manifest.api,
            host: ApiVersion::CURRENT,
        });
    }
    let Some(daemon) = &manifest.daemon else {
        return Ok(());
    };
    if package_file(package, &daemon.command).is_file() {
        return Ok(());
    }
    Err(InstallError::MissingProgram {
        command: daemon.command.clone(),
    })
}

/// Copies the regular files and the directories of a package. A package
/// with a different kind of file, for example a symbolic link, is refused:
/// a link can point out of the package.
pub(super) fn copy(source: &Path, destination: &Path) -> Result<(), InstallError> {
    fs::create_dir(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let target = destination.join(entry.file_name());
        if kind.is_dir() {
            copy(&entry.path(), &target)?;
        } else if kind.is_file() {
            fs::copy(entry.path(), target)?;
        } else {
            return Err(InstallError::UnsupportedFile {
                path: entry.path().display().to_string(),
            });
        }
    }
    Ok(())
}
