//! The SSH transport: a client inside nomux that reaches the daemon of a
//! remote host.
//!
//! - [`SshTarget`]: the destination that the user gives.
//! - [`SshEnvironment`]: the home directory, the name and the agent of the
//!   local user.
//! - [`config`]: the file `~/.ssh/config` of the user.
//! - [`known_hosts`]: the trust rules for the key of a host.
//! - [`SshConnection`]: one connection. Each connection to the daemon is a
//!   channel to the Unix socket of the daemon on the host. A command on the
//!   host has a channel of its own.
//!
//! `docs/remote.md` describes the channels, the supported configuration and
//! the trust rules.
//!
//! This module stores no secret: a key stays in ssh-agent or in its file,
//! and the module has no password. It never asks the host for agent
//! forwarding.

mod authentication;
pub mod config;
mod connection;
mod environment;
mod error;
mod host_key_check;
pub mod known_hosts;
mod pattern;
mod remote_command;
mod target;

pub use self::connection::SshConnection;
pub use self::environment::SshEnvironment;
pub use self::error::SshError;
pub use self::remote_command::CommandOutput;
pub use self::target::SshTarget;
