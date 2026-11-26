//! What the daemon reads of the host for the worktrees: the current
//! directories of the processes, the device of a directory, the free space
//! of a file system and whether a directory is writable.

use std::fs;
#[cfg(target_os = "linux")]
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process;

use nomux_holder_protocol::discovery::ProcessIdentity;
use rustix::fs::{Access, access};
#[cfg(target_os = "linux")]
use tracing::debug;

use crate::session_memory::process_list;

/// The current directory of each process that the daemon may read, without
/// the daemon and its children: the holders and the extensions have the
/// current directory of the daemon, and a session has its own entry. On
/// Linux `/proc/<pid>/cwd`, on macOS `proc_pidinfo`. On a different system
/// the list is empty: the daemon does not see the processes there.
pub(super) fn process_directories() -> Vec<PathBuf> {
    let Ok(pids) = process_list::pids() else {
        return Vec::new();
    };
    let daemon = process::id();
    pids.into_iter()
        .filter(|pid| {
            *pid != daemon
                && ProcessIdentity::with_parent_of(*pid)
                    .ok()
                    .flatten()
                    .is_some_and(|(_, parent)| parent != daemon)
        })
        .filter_map(current_directory)
        .collect()
}

#[cfg(target_os = "linux")]
fn current_directory(pid: u32) -> Option<PathBuf> {
    fs::read_link(format!("/proc/{pid}/cwd")).ok()
}

/// The current directory of the process `pid`. `None` if the process ended
/// or belongs to a different user.
#[cfg(target_os = "macos")]
#[expect(unsafe_code)]
fn current_directory(pid: u32) -> Option<PathBuf> {
    use std::ffi::CStr;
    use std::mem::MaybeUninit;
    use std::os::unix::ffi::OsStrExt;

    let pid = libc::c_int::try_from(pid).ok()?;
    let mut info = MaybeUninit::<libc::proc_vnodepathinfo>::zeroed();
    let size = libc::c_int::try_from(size_of::<libc::proc_vnodepathinfo>())
        .expect("the size of proc_vnodepathinfo fits in an int");
    // SAFETY: `info` is a writable buffer of `size` bytes, and
    // `PROC_PIDVNODEPATHINFO` writes a `proc_vnodepathinfo` of `size` bytes
    // at most.
    let written = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDVNODEPATHINFO,
            0,
            info.as_mut_ptr().cast(),
            size,
        )
    };
    if written != size {
        return None;
    }
    // SAFETY: `proc_pidinfo` returned `size`, thus it wrote the full struct.
    // The buffer was zeroed, and each bit pattern is a valid
    // `proc_vnodepathinfo`.
    let info = unsafe { info.assume_init() };
    // The kernel ends the path with a zero byte; a zeroed buffer has one.
    let bytes: Vec<u8> = info
        .pvi_cdir
        .vip_path
        .iter()
        .flatten()
        .map(|byte| byte.cast_unsigned())
        .collect();
    let path = CStr::from_bytes_until_nul(&bytes).ok()?;
    let path = std::ffi::OsStr::from_bytes(path.to_bytes());
    (!path.is_empty()).then(|| PathBuf::from(path))
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn current_directory(_pid: u32) -> Option<PathBuf> {
    None
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

/// `true` when this user can make a directory in the parent directory of
/// `directory`: the trash place goes there.
pub(super) fn has_writable_parent(directory: &Path) -> bool {
    directory
        .parent()
        .is_some_and(|parent| access(parent, Access::WRITE_OK | Access::EXEC_OK).is_ok())
}

/// The device of the file system of `directory`. `None` when not known.
pub(super) fn device(directory: &Path) -> Option<u64> {
    fs::metadata(directory).ok().map(|metadata| metadata.dev())
}

/// Writes the changes of the file system of `directory` to the disk: XFS
/// counts the blocks of a deleted file as free only after it.
#[cfg(target_os = "linux")]
pub(super) fn flush(directory: &Path) {
    let flushed = fs::File::open(directory)
        .and_then(|file| rustix::fs::syncfs(&file).map_err(io::Error::from));
    if let Err(error) = flushed {
        // The reading of the free space is then lower, not wrong.
        debug!(directory = %directory.display(), %error, "the file system was not flushed");
    }
}

/// Other systems have no flush of one file system.
#[cfg(not(target_os = "linux"))]
pub(super) fn flush(_directory: &Path) {}

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
