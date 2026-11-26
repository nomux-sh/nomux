//! The sandbox of the command: the holder reads the plan of the daemon and
//! builds the limits before the command starts. `docs/sandbox.md` has the
//! rules.
//!
//! - Without a launcher, the child of the holder applies the limits between
//!   `fork` and `exec`.
//! - With a launcher, for example the memory limit, the limits would stop
//!   the launcher: `systemd-run` needs the bus of the user manager, which
//!   the sandbox must not reach. Thus the launcher starts this binary with
//!   `--enter-sandbox`, which applies the same plan to itself and starts
//!   the command in its place.
//!
//! Each plan file is removed before the command starts: the command cannot
//! read it.

use std::ffi::OsString;
use std::fs;
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use nomux_holder_protocol::sandbox::{FeatureSet, SandboxPlan};
use nomux_sandbox::Prepared;

/// The sandbox of one start of the command.
pub(crate) struct CommandSandbox {
    pub(crate) plan: SandboxPlan,
    /// How the limits reach the command.
    pub(crate) entry: SandboxEntry,
    pub(crate) enforced: FeatureSet,
    pub(crate) landlock_abi: u8,
}

/// How the limits reach the command.
pub(crate) enum SandboxEntry {
    /// The child of the holder applies them between `fork` and `exec`.
    Child(Prepared),
    /// A launcher starts the stub with the plan of this file, which the
    /// stub removes. The file is the holder's own copy: the daemon removes
    /// its file when the holder is ready, which can be before the stub runs.
    Stub(PathBuf),
}

impl CommandSandbox {
    /// Reads the plan at `path`, removes the file and builds the limits.
    /// With a launcher, it writes the plan for the stub to `stub_plan`.
    pub(crate) fn prepare(path: &Path, stub_plan: Option<&Path>) -> io::Result<Self> {
        let plan = read_plan(path);
        remove_plan(path)?;
        let plan = plan?;
        let limits = nomux_sandbox::prepare(&plan).map_err(sandbox_error)?;
        let enforced = limits.enforced();
        let entry = match stub_plan {
            Some(stub_plan) => {
                write_plan(stub_plan, &plan)?;
                SandboxEntry::Stub(stub_plan.to_owned())
            }
            None => SandboxEntry::Child(limits),
        };
        Ok(Self {
            enforced,
            landlock_abi: nomux_sandbox::probe().landlock_abi,
            entry,
            plan,
        })
    }
}

/// Writes the copy of the stub. A copy of an earlier holder of the session,
/// which a launcher that failed left, is removed first.
fn write_plan(path: &Path, plan: &SandboxPlan) -> io::Result<()> {
    remove_plan(path)?;
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(OWNER_READ_WRITE)
        .open(path)
        .and_then(|mut file| file.write_all(plan.encode().as_bytes()))
        .map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("sandbox: cannot write the plan {}: {error}", path.display()),
            )
        })
}

const OWNER_READ_WRITE: u32 = 0o600;

/// `--enter-sandbox`: applies the plan at `path` to this process, removes
/// the file and starts `command` in its place. Returns only on an error:
/// the command then does not start.
pub(crate) fn enter_and_exec(path: &Path, command: &[OsString]) -> io::Error {
    let plan = read_plan(path);
    if let Err(error) = remove_plan(path) {
        return error;
    }
    let limits = match plan.and_then(|plan| nomux_sandbox::prepare(&plan).map_err(sandbox_error)) {
        Ok(limits) => limits,
        Err(error) => return error,
    };
    let Some((program, arguments)) = command.split_first() else {
        return io::Error::new(io::ErrorKind::InvalidInput, "the command is missing");
    };
    if let Err(error) = limits.apply_to_this_process() {
        return io::Error::new(
            error.kind(),
            format!("sandbox: cannot apply the limits: {error}"),
        );
    }
    Command::new(program).args(arguments).exec()
}

fn read_plan(path: &Path) -> io::Result<SandboxPlan> {
    let text = fs::read_to_string(path).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("sandbox: cannot read the plan {}: {error}", path.display()),
        )
    })?;
    SandboxPlan::parse(&text)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, format!("sandbox: {error}")))
}

fn remove_plan(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(io::Error::new(
            error.kind(),
            format!(
                "sandbox: cannot remove the plan {}: {error}",
                path.display()
            ),
        )),
        _ => Ok(()),
    }
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "it is the argument of `map_err`"
)]
fn sandbox_error(error: nomux_sandbox::SandboxError) -> io::Error {
    io::Error::other(format!("sandbox: {error}"))
}
