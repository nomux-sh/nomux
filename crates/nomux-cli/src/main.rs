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
//! The binary prints its version for `--version` and does nothing else. The
//! commands are not written yet.

use std::process::ExitCode;

fn main() -> ExitCode {
    nomux_core::version_only_main(nomux_core::version_line!())
}
