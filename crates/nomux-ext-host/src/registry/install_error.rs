//! Why the host does not accept a package or cannot change its state.

use std::fmt;
use std::io;

use nomux_extension::manifest::ManifestError;
use nomux_extension::{ApiRequirement, ApiVersion};

/// The reason why a directory is not a package that the host can use.
#[derive(Debug)]
#[non_exhaustive]
pub enum InstallError {
    /// A file or a directory was not read or written.
    Io(io::Error),
    /// `extension.toml` is not valid.
    Manifest(ManifestError),
    /// The manifest asks for an extension API version that the host does not
    /// supply.
    IncompatibleApi {
        /// The requirement in the manifest.
        required: ApiRequirement,
        /// The extension API version of the host.
        host: ApiVersion,
    },
    /// The package does not have the program of its daemon part.
    MissingProgram {
        /// The `command` of the `[daemon]` table.
        command: String,
    },
    /// The package has a file that is not a regular file or a directory.
    UnsupportedFile {
        /// The path of the file in the package.
        path: String,
    },
    /// A permission was approved that the manifest does not ask for.
    NotRequested {
        /// The permission, for example `scopes: sessions:read`.
        permission: String,
    },
    /// An extension with this identifier is already installed.
    AlreadyInstalled {
        /// The identifier of the extension.
        id: String,
    },
    /// The name of the package directory is not the identifier in its
    /// manifest.
    DirectoryName {
        /// The identifier in the manifest.
        id: String,
    },
}

impl fmt::Display for InstallError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "the package was not read or written: {error}"),
            Self::Manifest(error) => error.fmt(formatter),
            Self::IncompatibleApi { required, host } => write!(
                formatter,
                "the extension needs the extension API version {required}, but the host supplies {host}"
            ),
            Self::MissingProgram { command } => write!(
                formatter,
                "the package does not have the program `{command}` of its daemon part"
            ),
            Self::UnsupportedFile { path } => write!(
                formatter,
                "`{path}` in the package is not a regular file or a directory"
            ),
            Self::NotRequested { permission } => write!(
                formatter,
                "the manifest does not ask for the permission `{permission}`"
            ),
            Self::AlreadyInstalled { id } => {
                write!(formatter, "the extension `{id}` is already installed")
            }
            Self::DirectoryName { id } => write!(
                formatter,
                "the name of the package directory is not the identifier `{id}` of the manifest"
            ),
        }
    }
}

impl std::error::Error for InstallError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Manifest(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for InstallError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// The reason why the host did not change the state of an extension.
#[derive(Debug)]
pub enum StateError {
    /// No extension with this identifier is installed.
    NotInstalled,
    /// The state file was not written.
    Io(io::Error),
}

impl fmt::Display for StateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotInstalled => formatter.write_str("the extension is not installed"),
            Self::Io(error) => write!(formatter, "the state file was not written: {error}"),
        }
    }
}

impl std::error::Error for StateError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::NotInstalled => None,
            Self::Io(error) => Some(error),
        }
    }
}
