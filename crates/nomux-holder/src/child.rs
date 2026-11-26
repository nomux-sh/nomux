//! The child command of the session.

use std::ffi::{OsStr, OsString};
use std::io;
use std::os::fd::OwnedFd;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};

use nomux_holder_protocol::wire::{ChildSignal, ChildStatus};
use rustix::process::{Pid, Signal, kill_process};

/// The child command, and its exit status after it ended.
pub(crate) struct ChildProcess {
    child: Child,
    status: ChildStatus,
}

impl ChildProcess {
    /// Starts `program` as the leader of a new session. `terminal` is the
    /// slave side of the pseudoterminal: it becomes the controlling terminal,
    /// the standard input, the standard output and the standard error of the
    /// child. The child gets the environment of the holder and
    /// `variables`.
    pub(crate) fn spawn(
        program: &OsStr,
        arguments: &[impl AsRef<OsStr>],
        variables: impl IntoIterator<Item = (&'static str, OsString)>,
        working_directory: Option<&Path>,
        terminal: OwnedFd,
    ) -> io::Result<Self> {
        let mut command = Command::new(program);
        command
            .args(arguments)
            .envs(variables)
            .stdin(Stdio::from(terminal.try_clone()?))
            .stdout(Stdio::from(terminal.try_clone()?))
            .stderr(Stdio::from(terminal));
        if let Some(directory) = working_directory {
            command.current_dir(directory);
        }
        take_controlling_terminal(&mut command);
        Ok(Self {
            child: command.spawn()?,
            status: ChildStatus::Running,
        })
    }

    pub(crate) fn pid(&self) -> u32 {
        self.child.id()
    }

    pub(crate) fn status(&self) -> ChildStatus {
        self.status
    }

    pub(crate) fn has_exited(&self) -> bool {
        self.status != ChildStatus::Running
    }

    /// Collects the exit status if the child ended. Returns `true` on the
    /// call that finds the end.
    pub(crate) fn reap(&mut self) -> io::Result<bool> {
        if self.has_exited() {
            return Ok(false);
        }
        let Some(exit_status) = self.child.try_wait()? else {
            return Ok(false);
        };
        self.status = child_status(exit_status);
        Ok(true)
    }

    /// Sends a signal to the child. After the exit status is collected, the
    /// process ID can belong to a different process, thus no signal is sent
    /// then.
    pub(crate) fn signal(&self, signal: ChildSignal) -> io::Result<()> {
        if self.has_exited() {
            return Err(io::Error::from(io::ErrorKind::NotFound));
        }
        let signal = match signal {
            ChildSignal::Hangup => Signal::HUP,
            ChildSignal::Interrupt => Signal::INT,
            ChildSignal::Terminate => Signal::TERM,
            ChildSignal::Kill => Signal::KILL,
        };
        kill_process(Pid::from_child(&self.child), signal)?;
        Ok(())
    }
}

fn child_status(exit_status: ExitStatus) -> ChildStatus {
    match (exit_status.code(), exit_status.signal()) {
        (Some(code), _) => ChildStatus::Exited { code },
        (None, Some(signal)) => ChildStatus::Signaled { signal },
        // `try_wait` reports only a process that ended, and such a process
        // has a code or a signal.
        (None, None) => ChildStatus::Exited { code: -1 },
    }
}

/// Makes the child the leader of a new session with the standard input as
/// its controlling terminal.
#[expect(unsafe_code)]
fn take_controlling_terminal(command: &mut Command) {
    // SAFETY: the closure runs in the child between `fork` and `exec`. It
    // makes only the `setsid` and `ioctl` system calls, which are
    // async-signal-safe. It does not allocate and takes no lock: the
    // conversion of the error number into `io::Error` stores an integer.
    unsafe {
        command.pre_exec(|| {
            rustix::process::setsid()?;
            rustix::process::ioctl_tiocsctty(rustix::stdio::stdin())?;
            Ok(())
        });
    }
}
