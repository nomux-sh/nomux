//! The benchmark of the VT library candidates.
//!
//! The runner builds one probe binary for each candidate, runs the
//! measurements and prints the report as Markdown. `bench/README.md` shows
//! how to run it and what it measures.

mod candidates;
mod machine;
mod measurement;
mod probe;
mod report;
mod workspace;

use std::error::Error;
use std::process::ExitCode;

use candidates::CANDIDATES;
use measurement::Outcome;
use workspace::Workspace;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("vt-bench: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let workspace = Workspace::locate();
    let mut measurements = Vec::new();
    let mut not_built = Vec::new();
    for candidate in &CANDIDATES {
        eprintln!("vt-bench: {}", candidate.package);
        match measurement::measure(&workspace, candidate)? {
            Outcome::Measured(measurement) => measurements.push(measurement),
            Outcome::NotBuilt { package, reason } => not_built.push(format!("{package}: {reason}")),
        }
    }
    println!("# VT library benchmark\n");
    machine::print(workspace.target());
    for line in &not_built {
        println!("- Not measured: {line}");
    }
    report::print(&measurements);
    Ok(())
}
