//! The holder side of the daemon: starts a holder, connects to it and keeps
//! the connection.

mod backoff;
mod connection;
mod launch;
mod supervisor;

pub(crate) use connection::{HolderLink, open};
pub(crate) use launch::{LaunchError, launch};
pub(crate) use supervisor::{KillRequest, Supervisor};
