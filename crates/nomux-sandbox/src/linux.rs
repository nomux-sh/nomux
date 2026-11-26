//! The sandbox on Linux: Landlock, `no_new_privs` and seccomp.

mod seccomp_program;
mod system_calls;

use std::io;
use std::os::fd::AsFd;
use std::path::Path;
use std::process::Command;

use nomux_holder_protocol::sandbox::{FeatureSet, SandboxFeature, SandboxPlan};
use rustix::fs::{Mode, OFlags};

use self::system_calls::{ChildLimits, RulesetAttributes};
use crate::{HostSupport, SandboxError};

/// Execute, write, read a file, read a directory: the bits 0 to 3 of the
/// file system rights.
const ACCESS_EXECUTE: u64 = 1 << 0;
const ACCESS_READ_FILE: u64 = 1 << 2;
const ACCESS_READ_DIR: u64 = 1 << 3;
/// `LANDLOCK_ACCESS_FS_REFER`, since interface version 2.
const ACCESS_REFER: u64 = 1 << 13;
/// `LANDLOCK_ACCESS_FS_RESOLVE_UNIX`, since interface version 9.
const ACCESS_RESOLVE_UNIX: u64 = 1 << 16;
/// The rights of a path that the command can read and run.
const READ_ACCESS: u64 = ACCESS_EXECUTE | ACCESS_READ_FILE | ACCESS_READ_DIR;
/// `LANDLOCK_ACCESS_NET_BIND_TCP` and `CONNECT_TCP`, since version 4.
const NET_TCP: u64 = 0b11;
/// `LANDLOCK_SCOPE_ABSTRACT_UNIX_SOCKET` and `SCOPE_SIGNAL`, since version 6.
const SCOPE_ALL: u64 = 0b11;
/// `LANDLOCK_RESTRICT_SELF_LOG_NEW_EXEC_ON`, since version 7: the audit log
/// of the kernel gets the denials of the command, which runs a new
/// program.
const LOG_NEW_EXEC_ON: u32 = 1 << 1;

/// The limits of a plan, built in the holder before the command starts.
#[derive(Debug)]
pub struct Prepared {
    limits: ChildLimits,
    enforced: FeatureSet,
}

impl Prepared {
    /// The features that apply to the command.
    #[must_use]
    pub fn enforced(&self) -> FeatureSet {
        self.enforced
    }

    /// Makes the child of `command` apply the limits between `fork` and
    /// `exec`. If a call fails there, the command does not start and the
    /// spawn gives the error.
    pub fn install(self, command: &mut Command) {
        self.limits.install(command);
    }

    /// Applies the limits to this process. The caller starts the command
    /// with `exec` right after it, and has one thread.
    ///
    /// # Errors
    ///
    /// Returns the error of the system call that failed.
    pub fn apply_to_this_process(&self) -> io::Result<()> {
        self.limits.enter()
    }
}

impl std::fmt::Debug for ChildLimits {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ChildLimits")
            .field("ruleset", &self.ruleset)
            .field("seccomp_instructions", &self.seccomp_program.len())
            .finish_non_exhaustive()
    }
}

pub(crate) fn probe() -> HostSupport {
    let name = rustix::system::uname();
    HostSupport {
        landlock_abi: system_calls::landlock_abi(),
        seccomp: system_calls::seccomp_available(),
        system: format!("linux {}", name.release().to_string_lossy()),
    }
}

pub(crate) fn features(support: &HostSupport) -> FeatureSet {
    let abi = support.landlock_abi;
    let mut features = FeatureSet::EMPTY;
    if abi >= 1 {
        features = features.with(SandboxFeature::Files);
    }
    if support.seccomp && seccomp_program::ARCHITECTURE.is_some() {
        features = features.with(SandboxFeature::NetworkOff);
    }
    if abi >= 6 {
        features = features
            .with(SandboxFeature::Signals)
            .with(SandboxFeature::AbstractSockets);
    }
    if abi >= 9 {
        features = features.with(SandboxFeature::UnixSockets);
    }
    features
}

pub(crate) fn detail(support: &HostSupport) -> String {
    let landlock = match support.landlock_abi {
        0 => "no landlock (the kernel has none, it is off, or a policy blocks it)".to_owned(),
        abi => format!("landlock ABI {abi}"),
    };
    let features = features(support);
    let missing = FeatureSet::from_iter(SandboxFeature::ALL).without(features);
    let mut text = format!("{}, {landlock}", support.system);
    if !features.is_empty() {
        text = format!("{text}: {}", features.names().join(", "));
    }
    if !missing.is_empty() {
        text = format!("{text}; missing: {}", missing.names().join(", "));
    }
    text
}

pub(crate) fn prepare(plan: &SandboxPlan, support: &HostSupport) -> Result<Prepared, SandboxError> {
    let available = features(support);
    let missing = plan.require.without(available);
    if !missing.is_empty() {
        return Err(SandboxError::Unenforceable {
            profile: plan.profile.clone(),
            missing,
            detail: detail(support),
        });
    }
    let mut enforced = available.without(FeatureSet::EMPTY.with(SandboxFeature::NetworkOff));
    if plan.network_off {
        enforced = enforced.with(SandboxFeature::NetworkOff);
    }
    let ruleset = (support.landlock_abi >= 1)
        .then(|| build_ruleset(plan, support.landlock_abi))
        .transpose()?;
    let seccomp_program = if plan.network_off {
        seccomp_program::network_off()
    } else {
        Vec::new()
    };
    let restrict_flags = if support.landlock_abi >= 7 {
        LOG_NEW_EXEC_ON
    } else {
        0
    };
    Ok(Prepared {
        limits: ChildLimits {
            ruleset,
            restrict_flags,
            seccomp_program,
        },
        enforced,
    })
}

/// The file system rights that the kernel of interface version `abi`
/// knows. A right that the rule set does not handle is always permitted.
fn file_rights(abi: u8) -> u64 {
    let mut rights = (1 << 13) - 1;
    for (version, right) in [(2, 13), (3, 14), (5, 15), (9, 16)] {
        if abi >= version {
            rights |= 1 << right;
        }
    }
    rights
}

fn build_ruleset(plan: &SandboxPlan, abi: u8) -> Result<std::os::fd::OwnedFd, SandboxError> {
    let handled = file_rights(abi);
    let attributes = RulesetAttributes {
        handled_access_fs: handled,
        handled_access_net: if plan.network_off && abi >= 4 {
            NET_TCP
        } else {
            0
        },
        scoped: if abi >= 6 { SCOPE_ALL } else { 0 },
    };
    let ruleset = system_calls::create_ruleset(&attributes)
        .map_err(|error| system_error("cannot make the landlock rule set", error))?;
    // A path that the command can change: each right but the connect to a
    // Unix socket of a process outside the sandbox. `REFER` lets a rename
    // move a file between two of these paths.
    let write_access = handled & !ACCESS_RESOLVE_UNIX;
    let rules = (plan.read.iter().map(|path| (path, READ_ACCESS)))
        .chain(
            plan.write
                .iter()
                .chain(&plan.temporary)
                .map(|path| (path, write_access)),
        )
        .chain(
            plan.unix
                .iter()
                .map(|path| (path, (READ_ACCESS | ACCESS_RESOLVE_UNIX) & handled)),
        );
    for (path, access) in rules {
        add_rule(&ruleset, path, access & handled & !refer_without(abi))?;
    }
    Ok(ruleset)
}

/// `REFER` is not a right of a kernel before interface version 2.
fn refer_without(abi: u8) -> u64 {
    if abi >= 2 { 0 } else { ACCESS_REFER }
}

/// Adds one rule. A path that does not exist, or that the user cannot
/// open, adds no rule: the command then has fewer rights, not more.
fn add_rule(ruleset: &std::os::fd::OwnedFd, path: &Path, access: u64) -> Result<(), SandboxError> {
    let file = match rustix::fs::open(path, OFlags::PATH | OFlags::CLOEXEC, Mode::empty()) {
        Ok(file) => file,
        Err(rustix::io::Errno::NOENT | rustix::io::Errno::ACCESS) => return Ok(()),
        Err(error) => {
            return Err(system_error(
                &format!("cannot open {}", path.display()),
                error.into(),
            ));
        }
    };
    let is_directory = rustix::fs::fstat(&file).is_ok_and(|status| {
        rustix::fs::FileType::from_raw_mode(status.st_mode) == rustix::fs::FileType::Directory
    });
    // The kernel refuses a right of a directory on a file: a file has only
    // the rights that act on the file itself.
    let access = if is_directory {
        access
    } else {
        access & FILE_ONLY_RIGHTS
    };
    system_calls::add_path_rule(ruleset.as_fd(), file.as_fd(), access).map_err(|error| {
        system_error(
            &format!("cannot add the rule for {}", path.display()),
            error,
        )
    })
}

/// Execute, write, read, truncate, `ioctl` on a device and the connect to a
/// Unix socket: the rights that act on a file and not on a directory.
const FILE_ONLY_RIGHTS: u64 =
    ACCESS_EXECUTE | (1 << 1) | ACCESS_READ_FILE | (1 << 14) | (1 << 15) | ACCESS_RESOLVE_UNIX;

fn system_error(context: &str, error: io::Error) -> SandboxError {
    SandboxError::System {
        context: context.to_owned(),
        error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rights_of_a_rule_set_follow_the_interface_version() {
        assert_eq!(file_rights(1), 0x1fff);
        assert_eq!(file_rights(2), 0x3fff);
        assert_eq!(file_rights(4), 0x7fff);
        assert_eq!(file_rights(8), 0xffff);
        assert_eq!(file_rights(9), 0x1_ffff);
    }

    #[test]
    fn the_features_of_a_host_follow_its_interface_version() {
        let host = |landlock_abi, seccomp| HostSupport {
            landlock_abi,
            seccomp,
            system: "linux".to_owned(),
        };
        assert!(features(&host(0, false)).is_empty());
        assert_eq!(features(&host(4, false)).names(), ["files"]);
        assert_eq!(
            features(&host(6, true)).names(),
            ["files", "network_off", "signals", "abstract_sockets"]
        );
        assert_eq!(features(&host(9, true)).bits(), 0b1_1111);
    }
}
