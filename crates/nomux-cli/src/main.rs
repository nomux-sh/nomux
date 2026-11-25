//! `nomux`: the command-line interface.
//!
//! # Responsibility
//!
//! The CLI reads the command line, calls the client library and prints the
//! result as text or as JSON.
//!
//! # Boundaries
//!
//! - The CLI contains no protocol code. The `nomux-client` crate owns it.
//! - The CLI does not talk to a holder. It talks to the daemon only.
//!
//! # Status
//!
//! Milestone M0 prints the version for `--version` and does nothing else.
//! Milestone M1 adds the first commands.

use std::process::ExitCode;

fn main() -> ExitCode {
    nomux_core::version_only_main(&nomux_core::build_info!(), std::env::args_os().skip(1))
}
