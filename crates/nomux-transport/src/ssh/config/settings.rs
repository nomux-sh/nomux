//! What `~/.ssh/config` gives for one host.

use std::path::PathBuf;
use std::time::Duration;

use crate::ssh::SshTarget;

/// What the client does with a host key that no file of known hosts has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrictHostKeyChecking {
    /// `yes` and `ask`, which is the default: the connection fails. nomux
    /// asks no question.
    Strict,
    /// `accept-new`: the client adds the key and continues.
    AcceptNew,
    /// `no` and `off`: the client adds the key and continues.
    Off,
}

/// Where the keys of an agent come from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdentityAgent {
    /// The socket of the `SSH_AUTH_SOCK` variable.
    Environment,
    /// A socket that the configuration names.
    Socket(PathBuf),
    /// `IdentityAgent none`: no agent.
    Disabled,
}

/// The settings of one connection, after the configuration and the
/// defaults.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostSettings {
    /// The host name or the address to connect to.
    pub host_name: String,
    /// The port.
    pub port: u16,
    /// The user on the host.
    pub user: String,
    /// The key files, in the sequence in which the client tries them.
    pub identity_files: Vec<PathBuf>,
    /// `true`: the client uses only the keys of `identity_files`, also from
    /// an agent.
    pub identities_only: bool,
    /// The agent.
    pub identity_agent: IdentityAgent,
    /// The hosts to connect through, the first one first.
    pub proxy_jump: Vec<SshTarget>,
    /// The files of known hosts of the user. The client adds a new key to
    /// the first one.
    pub user_known_hosts_files: Vec<PathBuf>,
    /// The files of known hosts of the computer.
    pub global_known_hosts_files: Vec<PathBuf>,
    /// The rule for a host key that is not known.
    pub strict_host_key_checking: StrictHostKeyChecking,
    /// The name of the host in the files of known hosts: `HostKeyAlias`,
    /// or the host name.
    pub host_key_alias: String,
    /// The longest time for the start of the connection.
    pub connect_timeout: Duration,
    /// The time without data after which the client asks the host for an
    /// answer. `None`: the client does not ask.
    pub server_alive_interval: Option<Duration>,
    /// The number of questions without an answer after which the client
    /// closes the connection.
    pub server_alive_count_max: usize,
}
