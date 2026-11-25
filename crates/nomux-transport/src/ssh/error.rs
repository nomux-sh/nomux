//! Why an SSH connection or an operation on it failed.

use std::error::Error;
use std::fmt;
use std::io;
use std::path::PathBuf;

use russh::ChannelOpenFailure;

/// An SSH connection cannot be made, or an operation on it failed.
#[derive(Debug)]
pub enum SshError {
    /// The destination is not `[user@]host` or `ssh://[user@]host[:port]`.
    InvalidTarget {
        /// The destination as the user wrote it.
        target: String,
        /// What is wrong.
        problem: &'static str,
    },
    /// The environment of the process does not give the home directory or
    /// the user name.
    Environment(&'static str),
    /// `~/.ssh/config` or a file that it includes cannot be used.
    Configuration {
        /// The file.
        file: PathBuf,
        /// The line of the file. 0 if the problem is the file.
        line: usize,
        /// What is wrong.
        problem: String,
    },
    /// The configuration of the host needs an option that the SSH client of
    /// nomux does not have.
    UnsupportedOption {
        /// The option, as OpenSSH writes it.
        option: String,
        /// The file that sets the option.
        file: PathBuf,
        /// The line of the file.
        line: usize,
    },
    /// The TCP connection to the host cannot be made.
    Connect {
        /// The host name or the address.
        host: String,
        /// The port.
        port: u16,
        /// The error of the operating system.
        source: io::Error,
    },
    /// The connection ended or does not answer.
    Disconnected(String),
    /// No file of known hosts has a key for the host.
    UnknownHostKey {
        /// The host as the files of known hosts name it.
        host: String,
        /// The key type, for example `ssh-ed25519`.
        key_type: String,
        /// The SHA-256 fingerprint of the key that the host sent.
        fingerprint: String,
        /// The OpenSSH command that shows the key and adds it.
        accept_command: String,
    },
    /// A file of known hosts has a different key for the host.
    ChangedHostKey {
        /// The host as the files of known hosts name it.
        host: String,
        /// The key type.
        key_type: String,
        /// The SHA-256 fingerprint of the key that the host sent.
        fingerprint: String,
        /// The file with the different key.
        file: PathBuf,
        /// The line of the file.
        line: usize,
    },
    /// A file of known hosts marks the key of the host as revoked.
    RevokedHostKey {
        /// The host as the files of known hosts name it.
        host: String,
        /// The file with the `@revoked` line.
        file: PathBuf,
        /// The line of the file.
        line: usize,
    },
    /// The host accepted no key of the user.
    Authentication {
        /// The user on the host.
        user: String,
        /// The host.
        host: String,
        /// What nomux tried and why each key was not usable.
        notes: Vec<String>,
    },
    /// The SSH server refuses the connection to a Unix socket.
    ForwardingRefused,
    /// Nothing listens on the Unix socket of the host.
    SocketUnavailable,
    /// The SSH server refused to run a command.
    CommandRefused,
    /// The SSH library reported a different failure.
    Protocol(String),
}

impl SshError {
    /// Returns `true` if the network or the connection failed. A new
    /// attempt can succeed then. For each other error, a new attempt gives
    /// the same result.
    #[must_use]
    pub fn is_connection_failure(&self) -> bool {
        matches!(self, Self::Connect { .. } | Self::Disconnected(_))
    }
}

impl From<russh::Error> for SshError {
    fn from(error: russh::Error) -> Self {
        use russh::Error as Russh;
        match error {
            Russh::ChannelOpenFailure(ChannelOpenFailure::AdministrativelyProhibited) => {
                Self::ForwardingRefused
            }
            Russh::ChannelOpenFailure(ChannelOpenFailure::ConnectFailed) => Self::SocketUnavailable,
            Russh::IO(_)
            | Russh::Disconnect
            | Russh::HUP
            | Russh::SendError
            | Russh::ConnectionTimeout
            | Russh::KeepaliveTimeout
            | Russh::InactivityTimeout
            | Russh::Elapsed(_)
            | Russh::Join(_) => Self::Disconnected(error.to_string()),
            error => Self::Protocol(error.to_string()),
        }
    }
}

impl fmt::Display for SshError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidTarget { target, problem } => write!(
                formatter,
                "`{target}` is not an SSH destination: {problem}. Use [user@]host or \
                 ssh://[user@]host[:port]"
            ),
            Self::Environment(problem) => formatter.write_str(problem),
            Self::Configuration {
                file,
                line: 0,
                problem,
            } => write!(formatter, "{}: {problem}", file.display()),
            Self::Configuration {
                file,
                line,
                problem,
            } => write!(formatter, "{} line {line}: {problem}", file.display()),
            Self::UnsupportedOption { option, file, line } => write!(
                formatter,
                "{} line {line} sets `{option}` for this host, and the SSH client of nomux does \
                 not support `{option}`. nomux cannot connect to this host with this \
                 configuration",
                file.display()
            ),
            Self::Connect { host, port, source } => {
                write!(formatter, "cannot connect to {host} port {port}: {source}")
            }
            Self::Disconnected(reason) => write!(formatter, "the SSH connection ended: {reason}"),
            Self::UnknownHostKey {
                host,
                key_type,
                fingerprint,
                accept_command,
            } => write!(
                formatter,
                "the host key of {host} is not known. The host sent the {key_type} key with the \
                 fingerprint {fingerprint}. If the fingerprint is correct, run \
                 `{accept_command}` and answer `yes`, then run nomux again"
            ),
            Self::ChangedHostKey {
                host,
                key_type,
                fingerprint,
                file,
                line,
            } => write!(
                formatter,
                "THE HOST KEY OF {host} CHANGED. {} line {line} has a different {key_type} key. \
                 The host sent the key with the fingerprint {fingerprint}. Someone can be \
                 between this computer and the host. nomux does not connect. If the host has a \
                 new key, remove the old one with `ssh-keygen -R '{host}' -f '{}'`",
                file.display(),
                file.display()
            ),
            Self::RevokedHostKey { host, file, line } => write!(
                formatter,
                "the host key of {host} is revoked: {} line {line}. nomux does not connect",
                file.display()
            ),
            Self::Authentication { user, host, notes } => {
                write!(
                    formatter,
                    "the host {host} accepted no key for the user {user}. nomux uses the keys \
                     of ssh-agent and the key files, and it does not ask for a password"
                )?;
                for note in notes {
                    write!(formatter, ". {note}")?;
                }
                Ok(())
            }
            Self::ForwardingRefused => formatter.write_str(
                "the SSH server refuses connections to Unix sockets. nomux needs them to reach \
                 its daemon. On the host, `AllowStreamLocalForwarding` must permit `local` \
                 (the default is `yes`) and `DisableForwarding` must be `no` in `sshd_config`, \
                 and the key in `authorized_keys` must not have `restrict` or \
                 `no-port-forwarding`",
            ),
            Self::SocketUnavailable => {
                formatter.write_str("nothing listens on the socket of the daemon on the host")
            }
            Self::CommandRefused => {
                formatter.write_str("the SSH server refused to run a command on the host")
            }
            Self::Protocol(reason) => write!(formatter, "the SSH connection failed: {reason}"),
        }
    }
}

impl Error for SshError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Connect { source, .. } => Some(source),
            _ => None,
        }
    }
}
