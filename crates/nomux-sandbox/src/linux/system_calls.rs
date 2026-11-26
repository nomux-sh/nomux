//! The system calls of the sandbox that have no safe interface: the three
//! Landlock calls, `PR_SET_SECCOMP` with a program, and `pre_exec`. This
//! module is the only `unsafe` code of the sandbox.
//!
//! The child part ([`ChildLimits::enter`]) runs between `fork` and `exec`.
//! There it makes system calls only: it allocates no memory and takes no
//! lock. Each value that it needs is ready before the `fork`.

use std::io;
use std::os::fd::{AsRawFd, BorrowedFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::process::CommandExt;
use std::process::Command;
use std::ptr;

/// `struct landlock_ruleset_attr` of `linux/landlock.h`. A kernel of an
/// older interface accepts the larger structure when the fields that it
/// does not know are 0.
#[repr(C)]
pub(super) struct RulesetAttributes {
    pub(super) handled_access_fs: u64,
    pub(super) handled_access_net: u64,
    pub(super) scoped: u64,
}

/// `struct landlock_path_beneath_attr`: the kernel reads it packed.
#[repr(C, packed)]
struct PathBeneathAttributes {
    allowed_access: u64,
    parent_fd: i32,
}

const LANDLOCK_CREATE_RULESET_VERSION: u32 = 1;
const LANDLOCK_RULE_PATH_BENEATH: u32 = 1;

/// The version of the Landlock interface of the kernel. 0 if the kernel
/// has none, if it is turned off, or if a policy blocks the call.
#[expect(unsafe_code)]
pub(super) fn landlock_abi() -> u8 {
    // SAFETY: with a null attribute pointer, a size of 0 and the flag
    // `VERSION`, the kernel reads no memory of the process and returns the
    // version or an error.
    let version = unsafe {
        libc::syscall(
            libc::SYS_landlock_create_ruleset,
            ptr::null::<RulesetAttributes>(),
            0_usize,
            LANDLOCK_CREATE_RULESET_VERSION,
        )
    };
    u8::try_from(version).unwrap_or(0)
}

/// `true` if the kernel can apply a seccomp program: it knows the action
/// that refuses a call with an error number.
#[expect(unsafe_code)]
pub(super) fn seccomp_available() -> bool {
    let action: u32 = 0x0005_0000;
    // SAFETY: `SECCOMP_GET_ACTION_AVAIL` reads one `u32` from the pointer,
    // which points to `action` for the time of the call.
    let result = unsafe {
        libc::syscall(
            libc::SYS_seccomp,
            libc::SECCOMP_GET_ACTION_AVAIL,
            0_u32,
            ptr::from_ref(&action),
        )
    };
    result == 0
}

/// Makes a Landlock rule set that handles the rights of `attributes`.
#[expect(unsafe_code)]
pub(super) fn create_ruleset(attributes: &RulesetAttributes) -> io::Result<OwnedFd> {
    // SAFETY: `attributes` is a valid `struct landlock_ruleset_attr` for the
    // time of the call, and the size is its size. The kernel only reads it.
    let fd = unsafe {
        libc::syscall(
            libc::SYS_landlock_create_ruleset,
            ptr::from_ref(attributes),
            size_of::<RulesetAttributes>(),
            0_u32,
        )
    };
    let fd = RawFd::try_from(fd).map_err(|_| io::Error::last_os_error())?;
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the kernel gave a new descriptor with `O_CLOEXEC` that nothing
    // else owns.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

/// Adds the rule "the rights `allowed_access` below `parent`" to `ruleset`.
#[expect(unsafe_code)]
pub(super) fn add_path_rule(
    ruleset: BorrowedFd<'_>,
    parent: BorrowedFd<'_>,
    allowed_access: u64,
) -> io::Result<()> {
    let rule = PathBeneathAttributes {
        allowed_access,
        parent_fd: parent.as_raw_fd(),
    };
    // SAFETY: `rule` is a valid packed `struct landlock_path_beneath_attr`
    // for the time of the call, and the two descriptors are open. The kernel
    // only reads the structure.
    let result = unsafe {
        libc::syscall(
            libc::SYS_landlock_add_rule,
            ruleset.as_raw_fd(),
            LANDLOCK_RULE_PATH_BENEATH,
            ptr::from_ref(&rule),
            0_u32,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// What the child applies to itself before `exec`: everything is ready, and
/// [`enter`](Self::enter) only makes system calls.
pub(super) struct ChildLimits {
    pub(super) ruleset: Option<OwnedFd>,
    pub(super) restrict_flags: u32,
    pub(super) seccomp_program: Vec<libc::sock_filter>,
}

impl ChildLimits {
    /// Applies the limits to the calling thread: `no_new_privs`, the
    /// Landlock rule set, then the seccomp program. A process with one
    /// thread is limited as a whole, and each process that it starts
    /// inherits the limits.
    #[expect(unsafe_code)]
    pub(super) fn enter(&self) -> io::Result<()> {
        rustix::thread::set_no_new_privs(true)?;
        if let Some(ruleset) = &self.ruleset {
            // SAFETY: the descriptor is an open Landlock rule set. The call
            // reads no memory of the process.
            let result = unsafe {
                libc::syscall(
                    libc::SYS_landlock_restrict_self,
                    ruleset.as_raw_fd(),
                    self.restrict_flags,
                )
            };
            if result != 0 {
                return Err(io::Error::last_os_error());
            }
        }
        if !self.seccomp_program.is_empty() {
            let program = libc::sock_fprog {
                // The program builder makes fewer than 64 instructions.
                len: u16::try_from(self.seccomp_program.len()).unwrap_or(u16::MAX),
                filter: self.seccomp_program.as_ptr().cast_mut(),
            };
            // SAFETY: `program` points to the instructions of a vector that
            // lives for the time of the call, with their number. The kernel
            // copies them and does not write them.
            let result = unsafe {
                libc::prctl(
                    libc::PR_SET_SECCOMP,
                    libc::SECCOMP_MODE_FILTER,
                    ptr::from_ref(&program),
                )
            };
            if result != 0 {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(())
    }

    /// Makes the child of `command` apply the limits before `exec`. A
    /// failure makes the start of the command fail with the error.
    #[expect(unsafe_code)]
    pub(super) fn install(self, command: &mut Command) {
        // SAFETY: the closure runs in the child between `fork` and `exec`.
        // `enter` makes only the `prctl` and `landlock_restrict_self` system
        // calls, which are async-signal-safe. It allocates no memory and
        // takes no lock: the instructions and the descriptor were made
        // before the `fork`, the `sock_fprog` is on the stack, and an error
        // is an integer in `io::Error`.
        unsafe {
            command.pre_exec(move || self.enter());
        }
    }
}
