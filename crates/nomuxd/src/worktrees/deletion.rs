//! The deletion of one build output directory. The directory gets a new
//! name in its parent directory first: a build never sees a half-deleted
//! directory, and a deletion that a stop of the daemon interrupted leaves a
//! directory with a name that the next clean finds.
//!
//! The deletion follows no symbolic link and does not go into a different
//! file system. It runs on a thread that can wait.

use std::fs;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

/// The part of the new name of a directory whose deletion runs.
const DELETING: &str = ".nomux-deleting-";

/// The new name of `directory` for its deletion, in the same parent
/// directory.
pub(super) fn deleting_name(directory: &Path, id: &str) -> PathBuf {
    let name = super::last_part(directory);
    directory.with_file_name(format!("{name}{DELETING}{id}"))
}

/// Deletes `directory` and its content. An entry on a different file system
/// stays, with the directories above it: the result is an error that names
/// it.
pub(super) fn delete(directory: &Path) -> io::Result<()> {
    let root = match fs::symlink_metadata(directory) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if !root.is_dir() {
        return fs::remove_file(directory);
    }
    let mut kept = Vec::new();
    delete_below(directory, root.dev(), &mut kept)?;
    match kept.first() {
        None => Ok(()),
        Some(path) => Err(io::Error::other(format!(
            "{} is on a different file system and stays",
            path.display()
        ))),
    }
}

/// Deletes the content of `directory` and the directory. `kept` gets each
/// entry on a different file system.
fn delete_below(directory: &Path, device: u64, kept: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.is_dir() {
            if metadata.dev() == device {
                delete_below(&path, device, kept)?;
            } else {
                kept.push(path);
            }
        } else {
            fs::remove_file(&path)?;
        }
    }
    match fs::remove_dir(directory) {
        Err(_) if !kept.is_empty() => Ok(()),
        removed => removed,
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::symlink;

    use super::*;

    #[test]
    fn the_deletion_removes_a_symbolic_link_and_not_its_target() {
        let directory = tempfile::tempdir().unwrap();
        let outside = directory.path().join("outside");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("keep.txt"), "keep").unwrap();
        let target = directory.path().join("target");
        fs::create_dir_all(target.join("debug/deps")).unwrap();
        fs::write(target.join("debug/deps/app"), "binary").unwrap();
        symlink(&outside, target.join("debug/link")).unwrap();
        let renamed = deleting_name(&target, "1");

        fs::rename(&target, &renamed).unwrap();
        delete(&renamed).unwrap();

        assert_eq!(renamed.file_name().unwrap(), "target.nomux-deleting-1");
        assert!(!target.exists());
        assert!(!renamed.exists());
        assert_eq!(
            fs::read_to_string(outside.join("keep.txt")).unwrap(),
            "keep"
        );
    }
}
