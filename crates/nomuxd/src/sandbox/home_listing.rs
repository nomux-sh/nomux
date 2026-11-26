//! "Each path of a directory but these": the rules that let a sandbox read
//! the home directory without its secret directories.
//!
//! A Landlock rule permits a directory with each file below it, and a rule
//! cannot take a part away. Thus the listing gives one path for each entry
//! of the directory that is not denied. For a denied path with more parts,
//! such as `.config/gh`, it goes into `.config` and gives its entries but
//! `gh`. An entry that is created after the start of the session has no
//! rule: the command cannot read it.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// The paths of `directory` that are not `denied` and not above a denied
/// path. Each denied path is relative to `directory`.
///
/// A symbolic link is given only if its target is not a denied path, not
/// below one and not above one: a rule follows the link.
pub(super) fn readable_paths(directory: &Path, denied: &[PathBuf]) -> io::Result<Vec<PathBuf>> {
    let denied_targets: Vec<PathBuf> = denied
        .iter()
        .filter_map(|path| fs::canonicalize(directory.join(path)).ok())
        .collect();
    let mut paths = Vec::new();
    list(directory, denied, &denied_targets, &mut paths)?;
    Ok(paths)
}

fn list(
    directory: &Path,
    denied: &[PathBuf],
    denied_targets: &[PathBuf],
    paths: &mut Vec<PathBuf>,
) -> io::Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let name = PathBuf::from(entry.file_name());
        if denied.contains(&name) {
            continue;
        }
        let deeper: Vec<PathBuf> = denied
            .iter()
            .filter_map(|path| path.strip_prefix(&name).ok())
            .filter(|rest| !rest.as_os_str().is_empty())
            .map(Path::to_path_buf)
            .collect();
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            if is_clear_of(&path, denied_targets) && deeper.is_empty() {
                paths.push(path);
            }
        } else if deeper.is_empty() {
            paths.push(path);
        } else if file_type.is_dir() {
            list(&path, &deeper, denied_targets, paths)?;
        }
    }
    Ok(())
}

/// `true` if the target of the link `path` exists and is not a denied path,
/// not below one and not above one.
fn is_clear_of(path: &Path, denied_targets: &[PathBuf]) -> bool {
    let Ok(target) = fs::canonicalize(path) else {
        return false;
    };
    denied_targets
        .iter()
        .all(|denied| !target.starts_with(denied) && !denied.starts_with(&target))
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::symlink;

    use super::*;

    #[test]
    fn a_denied_path_and_the_links_into_it_are_not_listed() {
        let home = tempfile::tempdir().unwrap();
        let home = home.path();
        for directory in [".ssh", ".config/gh", ".config/git", "src"] {
            fs::create_dir_all(home.join(directory)).unwrap();
        }
        fs::write(home.join(".gitconfig"), "").unwrap();
        symlink(home.join(".ssh"), home.join("keys")).unwrap();
        symlink(home.join(".config"), home.join("settings")).unwrap();
        symlink(home.join("src"), home.join("code")).unwrap();
        let denied = [PathBuf::from(".ssh"), PathBuf::from(".config/gh")];

        let mut paths = readable_paths(home, &denied).unwrap();
        paths.sort();

        let expected: Vec<PathBuf> = [".config/git", ".gitconfig", "code", "src"]
            .iter()
            .map(|path| home.join(path))
            .collect();
        assert_eq!(paths, expected);
    }
}
