//! The control socket: the connections of the daemon and their requests.

mod connection;
mod peer;
mod server;

pub(crate) use server::{ControlServer, MAX_CONNECTIONS};
