//! The write of a settings file: a complete new file or nothing.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use tempfile::NamedTempFile;

use crate::ClientError;

/// Writes `content` to the file at `path`, or removes the file for `None`.
/// The new content goes to a temporary file in the same directory, which
/// then replaces the file. If the file is a symbolic link, the target of
/// the link changes and the link stays. The file keeps its permissions.
pub(super) fn write(path: &Path, content: Option<&[u8]>) -> Result<(), ClientError> {
    let target = link_target(path);
    let Some(content) = content else {
        return match fs::remove_file(&target) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(failure(&target, &error)),
            _ => Ok(()),
        };
    };
    let directory = target.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(directory).map_err(|error| failure(directory, &error))?;
    let mut file = NamedTempFile::new_in(directory).map_err(|error| failure(&target, &error))?;
    file.write_all(content)
        .map_err(|error| failure(&target, &error))?;
    if let Ok(metadata) = fs::metadata(&target) {
        fs::set_permissions(file.path(), metadata.permissions())
            .map_err(|error| failure(&target, &error))?;
    }
    file.persist(&target)
        .map_err(|error| failure(&target, &error.error))?;
    Ok(())
}

/// The file that a write changes: the target of a symbolic link.
fn link_target(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_owned())
}

pub(super) fn failure(path: &Path, error: &io::Error) -> ClientError {
    ClientError::Failed(format!("{}: {error}", path.display()))
}
