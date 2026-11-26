//! What the daemon reads of the host for the worktree list: the current
//! directories of the processes, the device of a directory and the free
//! space of a file system.

use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process;

use nomux_holder_protocol::discovery::ProcessIdentity;

/// The current directory of each process that the daemon may read, without
/// the daemon and its children: the holders and the extensions have the
/// current directory of the daemon, and a session has its own entry. On
/// Linux, `/proc/<pid>/cwd`. On a different system the list is empty: the
/// daemon does not see the processes there.
pub(super) fn process_directories() -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir("/proc") else {
        return Vec::new();
    };
    let daemon = process::id();
    entries
        .flatten()
        .filter_map(|entry| {
            let pid: u32 = entry.file_name().to_str()?.parse().ok()?;
            let (_, parent) = ProcessIdentity::with_parent_of(pid).ok()??;
            if pid == daemon || parent == daemon {
                return None;
            }
            fs::read_link(entry.path().join("cwd")).ok()
        })
        .collect()
}

/// `true` when `directory` exists and has a different device than its parent
/// directory: it is on a different file system, or it is a mount point.
pub(super) fn is_on_other_file_system(directory: &Path) -> bool {
    let Some(parent) = directory.parent() else {
        return false;
    };
    match (fs::metadata(directory), fs::metadata(parent)) {
        (Ok(directory), Ok(parent)) => directory.dev() != parent.dev(),
        _ => false,
    }
}

/// The free bytes for a user without special rights and the size of the file
/// system of `directory`. `(0, 0)` when not known.
pub(super) fn space(directory: &Path) -> (u64, u64) {
    match rustix::fs::statvfs(directory) {
        Ok(stat) => (
            stat.f_bavail.saturating_mul(stat.f_frsize),
            stat.f_blocks.saturating_mul(stat.f_frsize),
        ),
        Err(_) => (0, 0),
    }
}
