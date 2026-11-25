//! The control socket: the connections of the daemon and their requests.

mod connection;
mod peer;
mod redraw;
mod server;

pub(crate) use server::{ControlServer, MAX_CONNECTIONS};
