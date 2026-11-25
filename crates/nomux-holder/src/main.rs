//! `nomux-holder`: the session holder.
//!
//! # Responsibility
//!
//! One holder owns the PTY of one session. It always reads the PTY, and it
//! gives the output to the VT parser, to the journal and to a shared ring
//! buffer that the daemon reads. The holder stays alive when the daemon
//! stops, crashes or is updated, thus the session stays alive.
//!
//! # Boundaries
//!
//! - The holder has no async runtime. It must not depend on `tokio` or on a
//!   crate that starts a runtime.
//! - The holder does not talk to clients. Only the daemon connects to it.
//! - The holder stays small. The targets are in `bench/README.md`.
//!
//! # Status
//!
//! Milestone M0 prints the version for `--version` and does nothing else.
//! Milestone M1 adds the holder.

use std::process::ExitCode;

fn main() -> ExitCode {
    nomux_core::version_only_main(&nomux_core::build_info!(), std::env::args_os().skip(1))
}
