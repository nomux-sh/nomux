//! Support for the end-to-end tests: a private home directory, the `nomux`
//! CLI, a terminal for the commands that need one, and the processes of the
//! daemon and the holders.
//!
//! The tests run the binaries that Cargo built into the target directory.
//! `tests/README.md` says how to run them.

mod extension_package;
mod key_latency;
mod remote_host;
mod ssh_server;
mod tcp_relay;
mod terminal_client;

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use nomux_holder_protocol::discovery::{
    DiscoveredHolder, HolderState, PID_FILE, ProcessIdentity, scan,
};
use rustix::process::{Pid, Signal};
use serde_json::Value;
use tempfile::TempDir;

pub use crate::extension_package::extension_package;
pub use crate::key_latency::{key_to_echo, latency_row};
pub use crate::remote_host::RemoteHost;
pub use crate::ssh_server::{SshServer, generate_key, public_key, sshd_path};
pub use crate::tcp_relay::{Impairment, TcpRelay};
pub use crate::terminal_client::TerminalClient;

/// The longest time that a test waits for a condition.
pub(crate) const TIMEOUT: Duration = Duration::from_secs(20);

/// The path of a nomux binary in the target directory of this build.
///
/// # Panics
///
/// Panics if the binary does not exist: Cargo builds it only together with
/// the tests of its own package.
#[must_use]
pub fn binary(name: &str) -> PathBuf {
    // Cargo puts the binaries into `target/<profile>` and the test binary
    // into a directory below it. The depth of that directory is not the
    // same in each Cargo version.
    let test_binary = std::env::current_exe().expect("the test binary has a path");
    test_binary
        .ancestors()
        .map(|directory| directory.join(name))
        .find(|path| path.is_file())
        .unwrap_or_else(|| {
            panic!(
                "{name} is not in a directory above {}. Run `cargo build --workspace` first, \
                 or run the tests with `cargo nextest run --workspace`.",
                test_binary.display()
            )
        })
}

/// Waits until `condition` gives a value.
///
/// # Panics
///
/// Panics after 20 seconds without a value.
pub fn wait_for<T>(what: &str, mut condition: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        if let Some(value) = condition() {
            return value;
        }
        assert!(Instant::now() < deadline, "timeout: {what}");
        thread::sleep(Duration::from_millis(10));
    }
}

/// Returns `true` if a process with `pid` runs. A zombie does not run.
///
/// # Panics
///
/// Panics if the operating system does not give the state of the process.
#[must_use]
pub fn process_runs(pid: u32) -> bool {
    ProcessIdentity::of(pid)
        .expect("the process table can be read")
        .is_some()
}

/// Sends `SIGKILL` to the process `pid`.
///
/// # Panics
///
/// Panics if the signal cannot be sent.
pub fn kill_process(pid: u32) {
    rustix::process::kill_process(raw_pid(pid), Signal::KILL).expect("the process gets SIGKILL");
}

/// Sends `SIGKILL` to each process of the process group `process_group`.
///
/// # Panics
///
/// Panics if the signal cannot be sent.
pub fn kill_process_group(process_group: u32) {
    rustix::process::kill_process_group(raw_pid(process_group), Signal::KILL)
        .expect("the process group gets SIGKILL");
}

pub(crate) fn raw_pid(pid: u32) -> Pid {
    i32::try_from(pid)
        .ok()
        .and_then(Pid::from_raw)
        .expect("a process ID is a positive number")
}

/// The resident memory of the process `pid` in kibibytes.
///
/// # Panics
///
/// Panics if `ps` does not give the value.
#[must_use]
pub fn resident_memory_kib(pid: u32) -> u64 {
    let output = Command::new("ps")
        .args(["-o", "rss=", "-p", &pid.to_string()])
        .output()
        .expect("ps starts");
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .unwrap_or_else(|_| panic!("ps gives the memory of process {pid}: {output:?}"))
}

/// The result of one run of the `nomux` CLI.
#[derive(Debug)]
pub struct CliRun {
    /// The exit code.
    pub code: i32,
    /// The standard output.
    pub stdout: String,
    /// The standard error.
    pub stderr: String,
}

impl CliRun {
    /// The standard output as one JSON document.
    ///
    /// # Panics
    ///
    /// Panics if the standard output is not JSON.
    #[must_use]
    pub fn json(&self) -> Value {
        serde_json::from_str(&self.stdout)
            .unwrap_or_else(|error| panic!("the output is not JSON ({error}): {self:?}"))
    }
}

impl From<Output> for CliRun {
    fn from(output: Output) -> Self {
        Self {
            code: output.status.code().expect("the CLI ended with a code"),
            stdout: String::from_utf8(output.stdout).expect("the output is UTF-8"),
            stderr: String::from_utf8(output.stderr).expect("the output is UTF-8"),
        }
    }
}

/// A home directory that only one test uses. The daemon and the holders of
/// the test have their runtime directory in it.
///
/// When the value is dropped, it stops the daemon and the holders that the
/// test left.
#[derive(Debug)]
pub struct Sandbox {
    home: TempDir,
    /// The variables that each `nomux` process of the sandbox gets.
    variables: Vec<(OsString, OsString)>,
}

impl Default for Sandbox {
    fn default() -> Self {
        Self::new()
    }
}

impl Sandbox {
    /// Makes an empty home directory.
    ///
    /// # Panics
    ///
    /// Panics if the directory cannot be made.
    #[must_use]
    pub fn new() -> Self {
        // A short path: the path of a Unix socket has about 100 bytes at
        // most.
        let home = tempfile::Builder::new()
            .prefix("nomux")
            .tempdir_in("/tmp")
            .expect("a temporary directory can be made");
        Self {
            home,
            variables: Vec::new(),
        }
    }

    /// Gives each `nomux` process that starts after this call the variable
    /// `name` with `value`.
    pub fn set_variable(&mut self, name: &str, value: impl Into<OsString>) {
        self.variables.retain(|(existing, _)| existing != name);
        self.variables.push((name.into(), value.into()));
    }

    /// The runtime directory in this home directory.
    #[must_use]
    pub fn runtime_directory(&self) -> PathBuf {
        self.home.path().join(".nomux").join("run")
    }

    /// Runs `nomux` with `arguments` in this home directory.
    ///
    /// # Panics
    ///
    /// Panics if the CLI cannot start.
    #[must_use]
    pub fn nomux(&self, arguments: &[&str]) -> CliRun {
        self.nomux_command(arguments)
            .stdin(Stdio::null())
            .output()
            .expect("the CLI starts")
            .into()
    }

    /// Starts `nomux` with `arguments` in a terminal of 24 rows and 80
    /// columns.
    ///
    /// # Panics
    ///
    /// Panics if the terminal or the CLI cannot start.
    #[must_use]
    pub fn nomux_in_terminal(&self, arguments: &[&str]) -> TerminalClient {
        self.nomux_in_window(arguments, 24, 80)
    }

    /// Starts `nomux` with `arguments` in a terminal with a window of
    /// `rows` and `columns`.
    ///
    /// # Panics
    ///
    /// Panics if the terminal or the CLI cannot start.
    #[must_use]
    pub fn nomux_in_window(&self, arguments: &[&str], rows: u16, columns: u16) -> TerminalClient {
        TerminalClient::start(self.nomux_command(arguments), rows, columns)
    }

    fn nomux_command(&self, arguments: &[&str]) -> Command {
        let mut command = Command::new(binary("nomux"));
        command
            .args(arguments)
            .env("HOME", self.home.path())
            .env("SHELL", "/bin/sh")
            .env_remove("NOMUX_LOG")
            // The configuration file, the SSH keys and the binaries for a
            // remote host come only from the sandbox.
            .env_remove("XDG_CONFIG_HOME")
            .env_remove("SSH_AUTH_SOCK")
            .env_remove("NOMUX_BOOTSTRAP_BINARIES")
            .env_remove("NOMUX_RELEASE_DOWNLOAD_URL")
            .envs(self.variables.iter().map(|(name, value)| (name, value)))
            .current_dir(self.home.path());
        command
    }

    /// The home directory of the sandbox. A command of a session starts
    /// there.
    #[must_use]
    pub fn home(&self) -> &Path {
        self.home.path()
    }

    /// The socket of the daemon of this home directory.
    #[must_use]
    pub fn daemon_socket(&self) -> PathBuf {
        self.runtime_directory().join("nomuxd.sock")
    }

    /// Runs `nomux` and makes sure that it succeeded.
    ///
    /// # Panics
    ///
    /// Panics if the exit code is not 0.
    #[must_use]
    pub fn nomux_ok(&self, arguments: &[&str]) -> CliRun {
        let run = self.nomux(arguments);
        assert_eq!(run.code, 0, "nomux {arguments:?}: {run:?}");
        run
    }

    /// The process ID of the daemon that runs now.
    ///
    /// # Panics
    ///
    /// Panics if no daemon runs.
    #[must_use]
    pub fn daemon_pid(&self) -> u32 {
        let status = self.nomux_ok(&["daemon", "status", "--json"]).json();
        let pid = status["daemon"]["pid"].as_u64().expect("the daemon runs");
        u32::try_from(pid).expect("a process ID has 32 bits")
    }

    /// The sessions of `nomux ls --json`. The command starts a daemon if
    /// none runs.
    ///
    /// # Panics
    ///
    /// Panics if the command fails.
    #[must_use]
    pub fn sessions(&self) -> Vec<Value> {
        let list = self.nomux_ok(&["ls", "--json"]).json();
        list["sessions"]
            .as_array()
            .expect("the document has the sessions")
            .clone()
    }

    /// The holder directories of the runtime directory.
    ///
    /// # Panics
    ///
    /// Panics if the runtime directory cannot be read.
    #[must_use]
    pub fn holders(&self) -> Vec<DiscoveredHolder> {
        scan(&self.runtime_directory()).expect("the runtime directory can be read")
    }

    /// The process ID of the holder of the session `id`.
    ///
    /// # Panics
    ///
    /// Panics if the holder has no valid `pid` file.
    #[must_use]
    pub fn holder_pid(&self, id: &str) -> u32 {
        let pid_file = self.runtime_directory().join(id).join(PID_FILE);
        let identity: ProcessIdentity = std::fs::read_to_string(pid_file)
            .expect("the holder has a pid file")
            .parse()
            .expect("the pid file is valid");
        identity.pid
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = self.nomux(&["daemon", "stop"]);
        for holder in scan(&self.runtime_directory()).unwrap_or_default() {
            if holder.state != HolderState::Live && holder.state != HolderState::Starting {
                continue;
            }
            let holder_pid = self.holder_pid(holder.session_id.as_str());
            let _ = rustix::process::kill_process(raw_pid(holder_pid), Signal::KILL);
        }
    }
}
