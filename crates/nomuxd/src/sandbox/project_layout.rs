//! The project of a sandboxed session: the directory that the command can
//! change, and the parts of a git repository that a worktree needs.

use std::fs;
use std::path::{Path, PathBuf};

/// The directories of the project of one working directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ProjectLayout {
    /// The project directory: the closest directory at or above the working
    /// directory with a `.git`, or the working directory.
    pub(super) directory: PathBuf,
    /// For a git worktree: the parts of the common git directory that a
    /// commit changes. A hook or the configuration are not in it.
    pub(super) git_write: Vec<PathBuf>,
    /// For a git worktree: the common git directory, which git reads.
    pub(super) git_read: Option<PathBuf>,
}

impl ProjectLayout {
    /// The layout of `working_directory`. The search for `.git` stops
    /// below `home`: the home directory is never the project directory of a
    /// subdirectory.
    pub(super) fn of(working_directory: &Path, home: &Path) -> Self {
        let directory = working_directory
            .ancestors()
            .take_while(|directory| !home.starts_with(directory))
            .find(|directory| directory.join(".git").exists())
            .unwrap_or(working_directory)
            .to_owned();
        let mut layout = Self {
            directory,
            git_write: Vec::new(),
            git_read: None,
        };
        if let Some(worktree_git) = worktree_git_directory(&layout.directory) {
            let common = common_git_directory(&worktree_git);
            layout.git_write = ["objects", "refs", "logs"]
                .iter()
                .map(|part| common.join(part))
                .chain([worktree_git])
                .collect();
            layout.git_read = Some(common);
        }
        layout
    }
}

/// The git directory of a worktree: `.git` is a file with the line
/// `gitdir: <path>`.
fn worktree_git_directory(project: &Path) -> Option<PathBuf> {
    let text = fs::read_to_string(project.join(".git")).ok()?;
    let path = PathBuf::from(text.lines().next()?.strip_prefix("gitdir: ")?);
    Some(if path.is_absolute() {
        path
    } else {
        project.join(path)
    })
}

/// The common git directory of a worktree: the file `commondir`, or the
/// directory two levels above, as `<common>/worktrees/<name>`.
fn common_git_directory(worktree_git: &Path) -> PathBuf {
    match fs::read_to_string(worktree_git.join("commondir")) {
        Ok(text) => worktree_git.join(text.trim_end()),
        Err(_) => worktree_git
            .parent()
            .and_then(Path::parent)
            .unwrap_or(worktree_git)
            .to_owned(),
    }
}
