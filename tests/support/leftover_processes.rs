//! Finds and stops the processes that a test left behind.
//!
//! A test gives each process that it starts the home directory of its
//! sandbox, in the `HOME` variable or in an argument. A daemon, a holder
//! and the command of a session get it from their starter. Thus a process
//! of the sandbox is a process whose command line or environment has the
//! path of that directory, also when its parent is gone.

use std::io;
use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use rustix::process::Signal;

use crate::{TIMEOUT, raw_pid};

/// The time in which the processes that got a signal, or that lost their
/// terminal, end by themselves.
const GRACE: Duration = Duration::from_secs(2);
const POLL_INTERVAL: Duration = Duration::from_millis(20);

struct FoundProcess {
    pid: u32,
    /// The program and its arguments, without the environment: a variable
    /// can have a secret, and the text goes to the log of the tests.
    command: String,
}

/// Waits a short time for the processes of the sandbox `home` to end, and
/// stops the ones that still run with `SIGKILL`. Returns one line for each
/// process that it had to stop: a process that the test left behind.
///
/// # Errors
///
/// Returns an error if `ps` does not give the processes.
pub(crate) fn stop_leftover_processes(home: &Path) -> io::Result<Vec<String>> {
    let leftover = remaining_after(home, GRACE)?;
    for process in &leftover {
        // An error: the process ended a moment ago.
        let _ = rustix::process::kill_process(raw_pid(process.pid), Signal::KILL);
    }
    if !leftover.is_empty() && !remaining_after(home, TIMEOUT)?.is_empty() {
        return Err(io::Error::other(
            "a process of the sandbox did not end after SIGKILL",
        ));
    }
    Ok(leftover
        .into_iter()
        .map(|process| format!("process {}: {}", process.pid, process.command))
        .collect())
}

/// The processes of the sandbox that still run when `limit` is over.
/// Returns earlier when no process is left.
fn remaining_after(home: &Path, limit: Duration) -> io::Result<Vec<FoundProcess>> {
    let deadline = Instant::now() + limit;
    loop {
        let processes = processes_of(home)?;
        if processes.is_empty() || Instant::now() >= deadline {
            return Ok(processes);
        }
        thread::sleep(POLL_INTERVAL);
    }
}

fn processes_of(home: &Path) -> io::Result<Vec<FoundProcess>> {
    let home = home.to_string_lossy();
    // `e` adds the environment to the command line, and `ww` does not cut
    // the line. A zombie has no command line and no environment.
    let with_environment = process_listing("axeww")?;
    let commands = process_listing("axww")?;
    Ok(with_environment
        .into_iter()
        .filter(|(_, text)| text.contains(home.as_ref()))
        .map(|(pid, _)| {
            let command = commands.iter().find(|(listed, _)| *listed == pid);
            FoundProcess {
                pid,
                command: command.map_or_else(String::new, |(_, command)| command.clone()),
            }
        })
        .collect())
}

/// The process ID and the command column of each process, as `ps` with the
/// options `selection` gives them.
fn process_listing(selection: &str) -> io::Result<Vec<(u32, String)>> {
    let output = Command::new("ps")
        .args([selection, "-o", "pid=,command="])
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!("ps {selection} failed")));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.trim_start().split_once(char::is_whitespace))
        .filter_map(|(pid, command)| Some((pid.parse().ok()?, command.trim().to_owned())))
        .collect())
}
