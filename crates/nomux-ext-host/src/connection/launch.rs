//! Starts the process of the daemon part of an extension.

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::process::Stdio;

use tokio::process::{Child, Command};

/// What the host needs to start one process.
#[derive(Debug)]
pub(crate) struct Launch {
    /// The program of the daemon part.
    pub(crate) program: PathBuf,
    /// The working directory of the process. The host makes it.
    pub(crate) working_directory: PathBuf,
    /// All the environment variables of the process.
    pub(crate) environment: Vec<(OsString, OsString)>,
}

impl Launch {
    /// Starts the process with three pipes. The process does not get the
    /// environment of the daemon, and it ends when the host drops it.
    pub(super) fn spawn(&self) -> io::Result<Child> {
        fs::create_dir_all(&self.working_directory)?;
        Command::new(&self.program)
            .env_clear()
            .envs(self.environment.iter().map(|(name, value)| (name, value)))
            .current_dir(&self.working_directory)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
    }
}
