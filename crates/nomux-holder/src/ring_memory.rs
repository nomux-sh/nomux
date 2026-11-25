//! Makes the shared memory of the ring buffer.
//!
//! The memory has no name in the file system when this module returns. The
//! holder keeps a writable descriptor for its mapping and gives the read-only
//! descriptor to the daemon.

use std::io;
use std::os::fd::OwnedFd;

use rustix::fs::ftruncate;

pub(crate) struct RingMemory {
    pub(crate) writable: OwnedFd,
    pub(crate) read_only: OwnedFd,
}

/// Linux: an anonymous memory file. A second `open` through `/proc` gives a
/// descriptor that the kernel permits only to read and to map read-only.
#[cfg(target_os = "linux")]
pub(crate) fn create(length: usize) -> io::Result<RingMemory> {
    use std::os::fd::AsRawFd;

    use rustix::fs::{MemfdFlags, Mode, OFlags, memfd_create, open};

    let writable = memfd_create("nomux-ring", MemfdFlags::CLOEXEC)?;
    ftruncate(&writable, length as u64)?;
    let read_only = open(
        format!("/proc/self/fd/{}", writable.as_raw_fd()),
        OFlags::RDONLY | OFlags::CLOEXEC,
        Mode::empty(),
    )?;
    Ok(RingMemory {
        writable,
        read_only,
    })
}

/// macOS has no anonymous memory file. A POSIX shared memory object that is
/// unlinked immediately gives the same result: memory without a name that
/// lives as long as a descriptor or a mapping.
#[cfg(target_os = "macos")]
pub(crate) fn create(length: usize) -> io::Result<RingMemory> {
    use rustix::fs::Mode;
    use rustix::shm;

    let name = format!("/nomux-ring-{}", std::process::id());
    let owner_only = Mode::RUSR | Mode::WUSR;
    let create = shm::OFlags::CREATE | shm::OFlags::EXCL | shm::OFlags::RDWR;
    let writable = match shm::open(&name, create, owner_only) {
        // A holder with the same process ID ended before its `unlink`.
        Err(rustix::io::Errno::EXIST) => {
            shm::unlink(&name)?;
            shm::open(&name, create, owner_only)?
        }
        result => result?,
    };
    let read_only = shm::open(&name, shm::OFlags::RDONLY, Mode::empty());
    shm::unlink(&name)?;
    ftruncate(&writable, length as u64)?;
    Ok(RingMemory {
        writable,
        read_only: read_only?,
    })
}
