//! The sandbox of a nomux session.
//!
//! # Responsibility
//!
//! - [`probe`]: what the operating system of the host can apply.
//! - [`prepare`]: builds the limits of a [`SandboxPlan`] in the holder,
//!   before the command starts: it opens each path, makes the Landlock rule
//!   set and the seccomp program, and checks that the host applies each
//!   feature that the plan requires.
//! - [`Prepared::install`]: the child of the holder applies the limits
//!   between `fork` and `exec`, with system calls only.
//!
//! On Linux the limits are Landlock (files, Unix sockets, signals, abstract
//! sockets), `no_new_privs`, and a seccomp program for a plan without
//! network. On a different operating system, the host applies no feature,
//! and a plan that requires one does not start. `docs/sandbox.md` has the
//! rules.
//!
//! # Boundaries
//!
//! - No async runtime: the holder uses this crate.
//! - The `unsafe` code of the sandbox is in one module of this crate:
//!   `system_calls`.
//! - The crate never limits the process that calls [`prepare`]: only the
//!   child of [`Prepared::install`], or the caller of
//!   [`Prepared::apply_to_this_process`], which then starts the command
//!   with `exec`.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(not(target_os = "linux"))]
mod unsupported;

use std::error::Error;
use std::fmt;
use std::io;

use nomux_holder_protocol::sandbox::{FeatureSet, SandboxPlan};

#[cfg(target_os = "linux")]
use linux as platform;
#[cfg(target_os = "linux")]
pub use linux::Prepared;
#[cfg(not(target_os = "linux"))]
use unsupported as platform;
#[cfg(not(target_os = "linux"))]
pub use unsupported::Prepared;

/// What the operating system of the host can apply of a sandbox.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostSupport {
    /// The version of the Landlock interface of the kernel. 0 if the kernel
    /// has none, or if a policy of the system blocks it.
    pub landlock_abi: u8,
    /// The kernel can apply a seccomp program.
    pub seccomp: bool,
    /// The operating system and its version, for a person.
    pub system: String,
}

impl HostSupport {
    /// The features that the host can apply.
    #[must_use]
    pub fn features(&self) -> FeatureSet {
        platform::features(self)
    }

    /// What the host applies and what it does not, in English for a person.
    #[must_use]
    pub fn detail(&self) -> String {
        platform::detail(self)
    }
}

/// Asks the operating system what it can apply. The answer comes from the
/// system calls, not from the version of the kernel, thus it also finds a
/// container whose policy blocks them.
#[must_use]
pub fn probe() -> HostSupport {
    platform::probe()
}

/// Builds the limits of `plan` for one command.
///
/// # Errors
///
/// Returns [`SandboxError::Unenforceable`] if the host cannot apply a
/// feature that the plan requires, and [`SandboxError::System`] if a path
/// cannot be opened or a system call fails.
pub fn prepare(plan: &SandboxPlan) -> Result<Prepared, SandboxError> {
    platform::prepare(plan, &probe())
}

/// Why the limits of a plan cannot be built.
#[derive(Debug)]
pub enum SandboxError {
    /// The host cannot apply these features, which the plan requires.
    Unenforceable {
        /// The profile of the plan.
        profile: String,
        /// The features that the host does not apply.
        missing: FeatureSet,
        /// What the host applies, for a person.
        detail: String,
    },
    /// A system call failed.
    System {
        /// What failed.
        context: String,
        /// The error of the operating system.
        error: io::Error,
    },
}

impl fmt::Display for SandboxError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unenforceable {
                profile,
                missing,
                detail,
            } => write!(
                formatter,
                "this host cannot apply {} of the sandbox profile {profile} ({detail})",
                missing.names().join(", ")
            ),
            Self::System { context, error } => write!(formatter, "{context}: {error}"),
        }
    }
}

impl Error for SandboxError {}
