//! Protocol client of nomux.
//!
//! # Responsibility
//!
//! This crate is the client side of the nomux protocol. The CLI, the desktop
//! client and the mobile clients use it to connect to a daemon, to do the
//! handshake and to send requests.
//!
//! - [`Client`]: one connection to a daemon, with one method for each
//!   request.
//! - [`Client::attach`]: the terminal of a session. [`TerminalOutput`] gives
//!   the screen and the output as [`TerminalEvent`] values, and
//!   [`TerminalInput`] sends the input.
//! - [`ClientError`]: why a connection or a request failed.
//! - [`ClientKind`], [`Session`], [`SessionState`], [`NewSession`],
//!   [`WindowSize`] and [`HostInfo`]: the data of the requests and the
//!   responses.
//!
//! # Boundaries
//!
//! - This crate has no user interface and reads no command line. It does
//!   not read a keyboard and does not write to a terminal.
//! - This crate does not talk to a holder. Only the daemon talks to a holder.
//! - A caller needs no type of the `nomux-proto` crate.
//!
//! # Status
//!
//! The crate has the control requests for sessions and for the daemon on the
//! local host, and the attach to a session. Events and remote hosts are not
//! written yet.

mod client;
mod client_kind;
mod error;
mod host_info;
mod local_daemon;
mod session;
mod terminal;
mod window_size;

pub use client::Client;
pub use client_kind::ClientKind;
pub use error::ClientError;
pub use host_info::HostInfo;
pub use session::{NewSession, Session, SessionState};
pub use terminal::{
    AttachEnd, AttachOptions, OutputPosition, TerminalEvent, TerminalInput, TerminalOutput,
};
pub use window_size::WindowSize;
