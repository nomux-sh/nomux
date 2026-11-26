//! The memory limit of a session, as the operating system of the host can
//! apply it. This module is the one place that knows the difference of the
//! platforms. `docs/memory.md` has the rules.
//!
//! - Linux with a systemd user manager and control groups of version 2: the
//!   holder starts the command in a transient scope of the user manager with
//!   `MemoryMax`, `MemorySwapMax=0` and `OOMPolicy=kill`. The kernel ends
//!   each process of the session at the limit.
//! - Each other host: no operating system limit. The limit is a threshold
//!   for the hibernation of an idle agent session.

use std::ffi::OsString;
use std::path::PathBuf;

use nomux_holder_protocol::SessionId;
use tokio::process::Command;
use tracing::warn;

/// What the operating system of the host gives for a memory limit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LimitSupport {
    /// The kernel applies the limit: `systemd-run` starts the command in a
    /// scope of the user manager.
    Applied { systemd_run: PathBuf },
    /// No limit: the reason, in English for the user.
    Threshold { reason: String },
}

impl LimitSupport {
    /// What the host gives. The daemon asks one time, at its start: the
    /// checks read a few files and start no program.
    pub(crate) fn detect() -> Self {
        match applied_support() {
            Ok(systemd_run) => Self::Applied { systemd_run },
            Err(reason) => Self::Threshold { reason },
        }
    }

    pub(crate) fn is_applied(&self) -> bool {
        matches!(self, Self::Applied { .. })
    }

    /// The description for `Host.Info`.
    pub(crate) fn detail(&self) -> String {
        match self {
            Self::Applied { .. } => "the kernel ends the processes of a session at its limit: \
                                     a systemd user scope with MemoryMax"
                .to_owned(),
            Self::Threshold { reason } => format!(
                "the operating system applies no limit ({reason}); a limit is a threshold for \
                 the hibernation of an idle agent session"
            ),
        }
    }

    /// The words that start the command of the session `id` in its epoch
    /// `epoch` with the limit of `bytes`. Empty if the operating system
    /// applies no limit.
    pub(crate) fn launcher(&self, id: &SessionId, epoch: u32, bytes: u64) -> Vec<OsString> {
        let Self::Applied { systemd_run } = self else {
            return Vec::new();
        };
        let words = [
            "--user".to_owned(),
            "--scope".to_owned(),
            "--quiet".to_owned(),
            format!("--unit={}", unit_name(id, epoch)),
            format!("--property=MemoryMax={bytes}"),
            // Without it, a session at its limit fills the swap of the host.
            "--property=MemorySwapMax=0".to_owned(),
            // The kernel ends each process of the scope, not only one: a
            // command that lost one of its processes is in a state that
            // nobody tested.
            "--property=OOMPolicy=kill".to_owned(),
            "--".to_owned(),
        ];
        std::iter::once(systemd_run.clone().into_os_string())
            .chain(words.into_iter().map(OsString::from))
            .collect()
    }
}

/// `true` if the kernel ended the command of the session `id` in its epoch
/// `epoch` at the memory limit. The user manager keeps the failed scope with
/// its result; the function then removes it.
pub(crate) async fn limit_was_reached(id: &SessionId, epoch: u32) -> bool {
    let unit = unit_name(id, epoch);
    let result = Command::new("systemctl")
        .args(["--user", "show", "--property=Result", "--value", &unit])
        .output()
        .await;
    let reached = match result {
        Ok(output) => String::from_utf8_lossy(&output.stdout).trim() == "oom-kill",
        Err(error) => {
            warn!(session = %id, %error, "cannot ask the user manager how the session ended");
            return false;
        }
    };
    let reset = Command::new("systemctl")
        .args(["--user", "reset-failed", &unit])
        .status()
        .await;
    if let Err(error) = reset {
        warn!(session = %id, %error, "cannot remove the failed scope of the session");
    }
    reached
}

/// The memory limit that the kernel applies to the process `pid`, in bytes:
/// the `memory.max` of the scope of nomux that has the process. 0 if the
/// process is in no such scope.
#[cfg(target_os = "linux")]
pub(crate) fn kernel_limit(pid: u32) -> u64 {
    let Ok(membership) = std::fs::read_to_string(format!("/proc/{pid}/cgroup")) else {
        return 0;
    };
    // Version 2 has one line: `0::<path>`.
    let Some(path) = membership.lines().find_map(|line| line.strip_prefix("0::")) else {
        return 0;
    };
    let is_scope_of_nomux = path
        .rsplit('/')
        .next()
        .and_then(|name| name.strip_prefix("nomux-"))
        .is_some_and(|name| {
            name.rsplit_once('.')
                .is_some_and(|(_, kind)| kind == "scope")
        });
    if !is_scope_of_nomux {
        return 0;
    }
    std::fs::read_to_string(format!("/sys/fs/cgroup{path}/memory.max"))
        .ok()
        .and_then(|text| text.trim().parse().ok())
        .unwrap_or(0)
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn kernel_limit(_pid: u32) -> u64 {
    0
}

fn unit_name(id: &SessionId, epoch: u32) -> String {
    format!("nomux-{id}-{epoch}.scope")
}

#[cfg(target_os = "linux")]
fn applied_support() -> Result<PathBuf, String> {
    use std::env;
    use std::path::Path;

    if !Path::new("/sys/fs/cgroup/cgroup.controllers").is_file() {
        return Err("the host has no control groups of version 2".to_owned());
    }
    let systemd_run = nomux_core::program_in_path("systemd-run")
        .ok_or_else(|| "the program systemd-run is not in PATH".to_owned())?;
    let has_bus = env::var_os("DBUS_SESSION_BUS_ADDRESS").is_some()
        || env::var_os("XDG_RUNTIME_DIR")
            .is_some_and(|directory| Path::new(&directory).join("bus").exists());
    if !has_bus {
        return Err(
            "the daemon cannot reach the systemd user manager: it has no \
             DBUS_SESSION_BUS_ADDRESS and no $XDG_RUNTIME_DIR/bus"
                .to_owned(),
        );
    }
    let uid = rustix::process::getuid().as_raw();
    let controllers =
        format!("/sys/fs/cgroup/user.slice/user-{uid}.slice/user@{uid}.service/cgroup.controllers");
    let has_memory = std::fs::read_to_string(controllers)
        .is_ok_and(|text| text.split_whitespace().any(|name| name == "memory"));
    if !has_memory {
        return Err("the systemd user manager has no memory controller".to_owned());
    }
    Ok(systemd_run)
}

#[cfg(not(target_os = "linux"))]
fn applied_support() -> Result<PathBuf, String> {
    Err("macOS has no limit for the memory of a process tree without privileges".to_owned())
}
