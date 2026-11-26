//! A process that a test starts directly and that must not live longer
//! than the test, also when an assertion of the test fails.

use std::os::unix::process::CommandExt;
use std::process::{Child, Command};

use rustix::process::Signal;

use crate::raw_pid;

/// A process in a process group of its own. When the value is dropped, each
/// process of the group gets `SIGKILL`: the process and the processes that
/// it started and that did not leave the group.
#[derive(Debug)]
pub struct ProcessGuard {
    process: Child,
}

impl ProcessGuard {
    /// Starts `command` as the leader of a new process group.
    ///
    /// # Panics
    ///
    /// Panics if the process cannot start.
    #[must_use]
    pub fn spawn(command: &mut Command) -> Self {
        let process = command
            .process_group(0)
            .spawn()
            .unwrap_or_else(|error| panic!("{command:?} does not start: {error}"));
        Self { process }
    }

    /// The process, for its output and its exit status.
    pub fn process(&mut self) -> &mut Child {
        &mut self.process
    }
}

impl Drop for ProcessGuard {
    fn drop(&mut self) {
        // After the exit status of the leader was collected, the number of
        // the group can belong to a different process: no signal then.
        if matches!(self.process.try_wait(), Ok(None)) {
            let group = raw_pid(self.process.id());
            let _ = rustix::process::kill_process_group(group, Signal::KILL);
            let _ = self.process.wait();
        }
    }
}
