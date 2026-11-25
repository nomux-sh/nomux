//! The control socket: the connections of the daemon and their requests.

mod connection;
mod journal;
mod peer;
mod program_reports;
mod redraw;
mod server;

pub(crate) use server::{ControlServer, MAX_CONNECTIONS};
