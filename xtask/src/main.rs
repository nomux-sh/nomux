//! Developer tasks of the nomux workspace.
//!
//! Run a task with `cargo xtask <task>`. The alias is in `.cargo/config.toml`.
//!
//! Milestone M0 has one task, `ci`. Later milestones add the build, package,
//! PGO and code generation tasks.

use std::env;
use std::ffi::OsString;
use std::path::Path;
use std::process::{Command, ExitCode};

const USAGE: &str = "\
Usage: cargo xtask <task>

Tasks:
  ci    Run the checks that each pull request must pass:
        cargo fmt, cargo clippy, cargo nextest
";

/// The checks of the `ci` task, in the sequence in which they run.
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

/// Runs each step of [`CI_STEPS`]. Stops at the first step that fails.
fn ci() -> ExitCode {
    // Cargo sets `CARGO` to its own path when it runs this binary.
    let cargo = env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"));
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap_or(Path::new("."));

    for step in CI_STEPS {
        eprintln!("xtask: cargo {}", step.join(" "));
        match Command::new(&cargo).args(*step).current_dir(root).status() {
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
