//! What the SSH client takes from the process that runs it.

use std::env;
use std::path::PathBuf;

use super::SshError;

/// The local user of an SSH connection: the home directory with the `.ssh`
/// directory, the user name, and the socket of the agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshEnvironment {
    /// The home directory. `~/.ssh/config`, the key files and the files of
    /// known hosts are below it.
    pub home: PathBuf,
    /// The name of the local user. It is the user on the host if the
    /// destination and the configuration give none.
    pub user: String,
    /// The socket of the agent: the `SSH_AUTH_SOCK` variable.
    pub agent_socket: Option<PathBuf>,
}

impl SshEnvironment {
    /// Reads the `HOME`, `USER` (or `LOGNAME`) and `SSH_AUTH_SOCK`
    /// variables of this process.
    ///
    /// # Errors
    ///
    /// Returns an error if `HOME` is not set, or if `USER` and `LOGNAME`
    /// are not set.
    pub fn of_process() -> Result<Self, SshError> {
        let variable = |name: &str| env::var_os(name).filter(|value| !value.is_empty());
        let home = variable("HOME").ok_or(SshError::Environment(
            "the HOME variable is not set, thus ~/.ssh has no path",
        ))?;
        let user = variable("USER")
            .or_else(|| variable("LOGNAME"))
            .and_then(|user| user.into_string().ok())
            .ok_or(SshError::Environment(
                "the USER variable is not set, thus the local user has no name",
            ))?;
        Ok(Self {
            home: PathBuf::from(home),
            user,
            agent_socket: variable("SSH_AUTH_SOCK").map(PathBuf::from),
        })
    }

    /// The directory of the SSH files of the user: `~/.ssh`.
    #[must_use]
    pub fn ssh_directory(&self) -> PathBuf {
        self.home.join(".ssh")
    }
}
