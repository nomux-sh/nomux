//! `nomuxd`: the nomux daemon.
//!
//! # Responsibility
//!
//! One daemon runs on each host. It finds and manages the session holders,
//! keeps the state (data model, status resolver, event log), starts the
//! extensions, and serves the clients through the transport layer.
//!
//! # Boundaries
//!
//! - The daemon does not own a PTY. A holder owns it, thus a daemon restart
//!   stops no session.
//! - This crate contains the process setup only. The logic is in the library
//!   crates.
//!
//! # Status
//!
//! The binary prints its version for `--version` and does nothing else. The
//! daemon is not written yet.

use std::process::ExitCode;

fn main() -> ExitCode {
    nomux_core::version_only_main(nomux_core::version_line!())
}
