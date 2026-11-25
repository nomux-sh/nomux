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
//! Milestone M0 prints the version for `--version` and does nothing else.
//! Milestone M1 adds the daemon.

use std::process::ExitCode;

fn main() -> ExitCode {
    nomux_core::version_only_main(&nomux_core::build_info!(), std::env::args_os().skip(1))
}
