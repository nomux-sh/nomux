//! Protocol client of nomux.
//!
//! # Responsibility
//!
//! This crate is the client side of the nomux protocol. The CLI, the desktop
//! client and the mobile clients use it to connect to a daemon, to do the
//! handshake and to send requests.
//!
//! - [`Host`]: the local host, or a remote host through SSH. It opens the
//!   connections to the daemon of the host and starts the daemon. For a
//!   remote host it also installs the nomux binaries: [`BinarySource`],
//!   [`Installation`] and [`BootstrapStep`].
//! - [`Client`]: one connection to a daemon, with one method for each
//!   request.
//! - [`Host::attach`]: the terminal of a session. [`TerminalOutput`] gives
//!   the screen and the output as [`TerminalEvent`] values, and
//!   [`TerminalInput`] sends the input. The attach connects again when its
//!   connection ends.
//! - [`check_host`]: the checks of a remote host, as [`HostCheck`] values.
//! - [`Extension`], [`ExtensionPackage`] and [`ExtensionPermissions`]: the
//!   extensions of a host. [`Client::install_extension`] installs one in two
//!   steps, and [`Client::call_extension`] calls a method of one.
//! - [`ClientError`]: why a connection or a request failed.
//! - [`Client::capture`] and [`Client::capture_lines`]: the scrollback of a
//!   session, without an attach.
//! - [`ClientKind`], [`Session`], [`SessionState`], [`SessionExit`],
//!   [`RestorePolicy`], [`NewSession`], [`WindowSize`] and [`HostInfo`]: the
//!   data of the requests and the responses.
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
//! The crate has the control requests for sessions, for extensions and for
//! the daemon, the attach to a session with the output of its journal, the
//! scrollback, the restore of a dormant session, and the two kinds of host:
//! the local host and a remote host through SSH. Events are not written yet.

mod bootstrap;
mod client;
mod client_kind;
mod doctor;
mod error;
mod extension;
mod extension_requests;
mod host;
mod host_info;
mod local_host;
mod reconnect;
mod scrollback;
mod session;
mod ssh_host;
mod terminal;
mod window_size;

pub use bootstrap::{BinarySource, BootstrapStep, Installation};
pub use client::Client;
pub use client_kind::ClientKind;
pub use doctor::{HostCheck, check_host};
pub use error::ClientError;
pub use extension::{
    Extension, ExtensionCallError, ExtensionErrorCode, ExtensionPackage, ExtensionPermissions,
    ExtensionState,
};
pub use host::Host;
pub use host_info::HostInfo;
pub use nomux_core::configuration::RestorePolicy;
pub use scrollback::{CaptureRange, CapturedOutput};
pub use session::{ExitStatus, NewSession, Session, SessionExit, SessionState};
pub use terminal::{
    AttachEnd, AttachOptions, OutputPosition, TerminalEvent, TerminalInput, TerminalOutput,
};
pub use window_size::WindowSize;
