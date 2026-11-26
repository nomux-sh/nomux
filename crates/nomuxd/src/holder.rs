//! The holder side of the daemon: starts a holder, connects to it and keeps
//! the connections.

mod backoff;
mod connection;
mod launch;
mod output_feed;
mod supervisor;

pub(crate) use connection::{HolderLink, open_with_lease};
pub(crate) use launch::{HolderStart, LaunchError, launch};
pub(crate) use output_feed::{OutputFeed, SharedOutputFeed};
pub(crate) use supervisor::{SessionCommand, SessionEnd, Supervisor, WriteError};
