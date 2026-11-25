//! The paths in the directory of the host.
//!
//! ```text
//! <directory>/
//!   packages/<id>/     the package of an extension, with its `extension.toml`
//!   state/<id>.toml    the state file of the extension
//!   data/<id>/         the working directory of the process
//! ```
//!
//! The state file is not in the package, thus a package cannot bring its
//! own approval.

use std::path::{Path, PathBuf};

#[derive(Debug)]
pub(super) struct Layout {
    directory: PathBuf,
}

impl Layout {
    pub(super) fn new(directory: PathBuf) -> Self {
        Self { directory }
    }

    pub(super) fn packages(&self) -> PathBuf {
        self.directory.join("packages")
    }

    pub(super) fn package(&self, id: &str) -> PathBuf {
        self.packages().join(id)
    }

    /// Where a package is while the host copies it. The name starts with
    /// `.`, which the identifier of an extension cannot do.
    pub(super) fn incoming_package(&self, id: &str) -> PathBuf {
        self.packages().join(format!(".incoming-{id}"))
    }

    pub(super) fn state_file(&self, id: &str) -> PathBuf {
        self.directory.join("state").join(format!("{id}.toml"))
    }

    pub(super) fn data(&self, id: &str) -> PathBuf {
        self.directory.join("data").join(id)
    }
}

/// The path of a file of a package. `relative` is a path that the manifest
/// validator accepted: it has `/` between its parts and cannot go out of the
/// package.
pub(super) fn package_file(package: &Path, relative: &str) -> PathBuf {
    relative
        .split('/')
        .fold(package.to_path_buf(), |path, part| path.join(part))
}
