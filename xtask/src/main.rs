//! Developer tasks of the nomux workspace.
//!
//! Run a task with `cargo xtask <task>`. The alias is in `.cargo/config.toml`.

mod fuzz;

use std::env;
use std::ffi::OsString;
use std::path::Path;
use std::process::{Command, ExitCode};

const USAGE: &str = "\
Usage: cargo xtask <task>

Tasks:
  ci                       Run the checks that CI runs on each pull request:
                           cargo fmt, cargo clippy, cargo nextest,
                           cargo test --doc, cargo deny, cargo machete
  fuzz <target> <seconds>  Run one fuzz target of the `fuzz` directory.
                           It needs cargo-fuzz and the nightly toolchain
                           of `fuzz/rust-toolchain.toml`.
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
    let args: Vec<String> = env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args[..] {
        ["ci"] => ci(),
        ["fuzz", target, seconds] => fuzz::run(root(), target, seconds),
        ["help" | "--help" | "-h"] => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        _ => {
            eprint!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

/// The root of the repository.
fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap_or(Path::new("."))
}

fn ci() -> ExitCode {
    // Cargo sets `CARGO` to its own path when it runs this binary.
    let cargo = env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"));
    let root = root();

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
