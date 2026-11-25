//! Developer tasks of the nomux workspace.
//!
//! Run a task with `cargo xtask <task>`. The alias is in `.cargo/config.toml`.

use std::env;
use std::ffi::OsString;
use std::path::Path;
use std::process::{Command, ExitCode};

const USAGE: &str = "\
Usage: cargo xtask <task>

Tasks:
  ci    Run the checks that CI runs on each pull request:
        cargo fmt, cargo clippy, cargo nextest, cargo test --doc,
        cargo deny, cargo machete
";

/// The checks of the `ci` task, in the sequence in which they run. The
/// commands are the same as in the reusable Rust workflow of the organization.
const CI_STEPS: &[&[&str]] = &[
    &["fmt", "--all", "--check"],
    &[
        "clippy",
        "--workspace",
        "--all-targets",
        "--",
        "-D",
        "warnings",
    ],
    &["nextest", "run", "--workspace"],
    &["test", "--workspace", "--doc"],
    &["deny", "check"],
    &["machete"],
];

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    match (args.next().as_deref(), args.next()) {
        (Some("ci"), None) => ci(),
        (Some("help" | "--help" | "-h"), None) => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        _ => {
            eprint!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn ci() -> ExitCode {
    // Cargo sets `CARGO` to its own path when it runs this binary.
    let cargo = env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"));
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap_or(Path::new("."));

    for step in CI_STEPS {
        eprintln!("xtask: cargo {}", step.join(" "));
        let status = Command::new(&cargo)
            .args(*step)
            .current_dir(root)
            // `cargo run` sets this variable for xtask. With it, cargo-machete
            // reads its subcommand name as a directory.
            .env_remove("CARGO_PKG_NAME")
            .status();
        match status {
            Ok(status) if status.success() => {}
            Ok(status) => {
                eprintln!("xtask: `cargo {}` failed: {status}", step.join(" "));
                return ExitCode::FAILURE;
            }
            Err(error) => {
                eprintln!("xtask: cannot start `cargo {}`: {error}", step.join(" "));
                return ExitCode::FAILURE;
            }
        }
    }
    eprintln!("xtask: all checks passed");
    ExitCode::SUCCESS
}
