//! What the daemon keeps of the worktrees on disk: the file `worktrees.json`
//! in the state directory, with the marks of the user, the entries of the
//! trash place and the build output directories whose deletion did not end.
//! `docs/worktrees.md` has the format.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use nomux_proto::wire::v1::{WorktreeMark, WorktreeTrashEntry};
use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::session_store::small_file;

const FILE: &str = "worktrees.json";
/// The version of the format. A daemon reads only the version that it knows.
const VERSION: u32 = 1;
/// Larger than the file of a host with thousands of worktrees.
const MAX_LENGTH: u64 = 4 * 1024 * 1024;

/// One worktree in the trash place.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct TrashEntry {
    pub(super) id: String,
    pub(super) project_id: String,
    /// The directory of the Project: the repository of the calls to the git
    /// extension.
    pub(super) repository: PathBuf,
    /// The directory before the move.
    pub(super) path: PathBuf,
    pub(super) trash_path: PathBuf,
    pub(super) branch: String,
    pub(super) head: String,
    pub(super) trashed_at_ms: u64,
    pub(super) expires_at_ms: u64,
    pub(super) deletes_branch: bool,
    /// The ignored paths that the removal may delete: the files of the
    /// user with `include_ignored`.
    pub(super) allowed_ignored_paths: Vec<String>,
    pub(super) allow_unpushed: bool,
}

impl TrashEntry {
    pub(super) fn wire(&self) -> WorktreeTrashEntry {
        WorktreeTrashEntry {
            id: self.id.clone(),
            project_id: self.project_id.clone(),
            path: self.path.display().to_string(),
            trash_path: self.trash_path.display().to_string(),
            branch: self.branch.clone(),
            head: self.head.clone(),
            trashed_at_ms: self.trashed_at_ms,
            expires_at_ms: self.expires_at_ms,
            deletes_branch: self.deletes_branch,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum StoredMark {
    Finished,
    Keep,
}

#[derive(Default, Serialize, Deserialize)]
struct Stored {
    version: u32,
    /// The mark of each worktree, by its directory.
    #[serde(default)]
    marks: BTreeMap<PathBuf, StoredMark>,
    #[serde(default)]
    trash: Vec<TrashEntry>,
    /// The renamed build output directories that the daemon deletes.
    #[serde(default)]
    deleting: Vec<PathBuf>,
}

pub(super) struct Store {
    directory: PathBuf,
    stored: Mutex<Stored>,
}

impl Store {
    /// The file of the state directory `directory`. A file that the daemon
    /// cannot read gives an empty store, and the next change replaces it.
    pub(super) fn open(directory: &Path) -> Self {
        let path = directory.join(FILE);
        let stored = match small_file::read(&path, MAX_LENGTH) {
            Ok(bytes) => serde_json::from_slice::<Stored>(&bytes)
                .ok()
                .filter(|stored| stored.version == VERSION)
                .unwrap_or_else(|| {
                    warn!(file = %path.display(), "the daemon cannot use the worktree file");
                    Stored::default()
                }),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Stored::default(),
            Err(error) => {
                warn!(file = %path.display(), %error, "the daemon cannot read the worktree file");
                Stored::default()
            }
        };
        Self {
            directory: directory.to_owned(),
            stored: Mutex::new(stored),
        }
    }

    /// The mark of each worktree that has one, by its directory.
    pub(super) fn marks(&self) -> HashMap<PathBuf, WorktreeMark> {
        self.stored()
            .marks
            .iter()
            .map(|(path, mark)| {
                let mark = match mark {
                    StoredMark::Finished => WorktreeMark::Finished,
                    StoredMark::Keep => WorktreeMark::Keep,
                };
                (path.clone(), mark)
            })
            .collect()
    }

    pub(super) fn set_mark(&self, path: &Path, mark: WorktreeMark) -> io::Result<()> {
        self.change(|stored| {
            match mark {
                WorktreeMark::Finished => {
                    stored.marks.insert(path.to_owned(), StoredMark::Finished)
                }
                WorktreeMark::Keep => stored.marks.insert(path.to_owned(), StoredMark::Keep),
                WorktreeMark::Unspecified => stored.marks.remove(path),
            };
        })
    }

    pub(super) fn trash_entries(&self) -> Vec<TrashEntry> {
        self.stored().trash.clone()
    }

    /// Adds an entry and removes the mark of its worktree.
    pub(super) fn add_trash_entry(&self, entry: TrashEntry) -> io::Result<()> {
        self.change(|stored| {
            stored.marks.remove(&entry.path);
            stored.trash.push(entry);
        })
    }

    pub(super) fn remove_trash_entry(&self, id: &str) -> io::Result<()> {
        self.change(|stored| stored.trash.retain(|entry| entry.id != id))
    }

    /// Removes the mark of a worktree that a clean removed.
    pub(super) fn forget(&self, path: &Path) -> io::Result<()> {
        self.change(|stored| {
            stored.marks.remove(path);
        })
    }

    pub(super) fn deleting(&self) -> Vec<PathBuf> {
        self.stored().deleting.clone()
    }

    pub(super) fn add_deleting(&self, path: &Path) -> io::Result<()> {
        self.change(|stored| stored.deleting.push(path.to_owned()))
    }

    pub(super) fn remove_deleting(&self, path: &Path) -> io::Result<()> {
        self.change(|stored| stored.deleting.retain(|deleting| deleting != path))
    }

    /// Changes the store and writes the file.
    fn change(&self, change: impl FnOnce(&mut Stored)) -> io::Result<()> {
        let mut stored = self.stored();
        change(&mut stored);
        stored.version = VERSION;
        let bytes = serde_json::to_vec_pretty(&*stored).map_err(io::Error::other)?;
        fs::create_dir_all(&self.directory)?;
        small_file::replace(&self.directory, FILE, &bytes)
    }

    fn stored(&self) -> MutexGuard<'_, Stored> {
        // No code panics while it holds the lock, thus the store is whole.
        self.stored.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
