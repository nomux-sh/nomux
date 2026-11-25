//! The report of a benchmark run: Markdown tables on the standard output.

use vt_probe::Corpus;

use crate::candidates::Kind;
use crate::measurement::{Measurement, SCROLLBACK_LINES};
use crate::probe::HeldTerminal;
use crate::workspace::PROFILES;

pub(crate) fn print(measurements: &[Measurement]) {
    print_throughput(measurements);
    print_memory(measurements);
    print_size(measurements);
    print_conformance(measurements);
}

fn print_throughput(measurements: &[Measurement]) {
    println!("\n## Parse throughput (MiB/s)\n");
    let corpora = Corpus::ALL.map(Corpus::name);
    print_header("Candidate", &corpora);
    for measurement in measurements.iter().filter(|m| m.kind != Kind::Baseline) {
        let cells = measurement
            .throughput
            .iter()
            .map(|rate| format!("{rate:.0}"));
        print_row(&measurement.name, cells);
    }
}

fn print_memory(measurements: &[Measurement]) {
    println!("\n## Memory of one session (resident set size in KiB)\n");
    let columns = SCROLLBACK_LINES.map(|lines| match lines {
        0 => "screen only".to_owned(),
        lines => format!("{lines} lines of scrollback"),
    });
    print_header("Candidate", &columns);
    let baseline = measurements.first();
    for measurement in measurements {
        let cells = measurement
            .terminals
            .iter()
            .zip(0..)
            .map(|(terminal, index)| {
                let reference =
                    baseline.map_or(0, |baseline| baseline.terminals[index].resident_kib);
                let resident = with_difference(terminal.resident_kib, reference, measurement.kind);
                format!("{resident}{}", kept_lines_note(terminal))
            });
        print_row(&measurement.name, cells);
    }
}

/// A note for a candidate that does not keep the lines that it was given.
fn kept_lines_note(terminal: &HeldTerminal) -> String {
    if terminal.retained_lines == terminal.scrollback_lines {
        return String::new();
    }
    format!(", keeps {} lines", terminal.retained_lines)
}

fn print_size(measurements: &[Measurement]) {
    println!("\n## Size of the stripped probe binary (bytes) and build requirements\n");
    let mut columns: Vec<String> = PROFILES
        .iter()
        .map(|profile| format!("profile `{profile}`"))
        .collect();
    columns.extend(["crates".to_owned(), "build tools".to_owned()]);
    print_header("Candidate", &columns);
    let baseline = measurements.first();
    for measurement in measurements {
        let sizes = measurement.sizes.iter().enumerate().map(|(index, size)| {
            let reference = baseline.map_or(0, |baseline| baseline.sizes[index]);
            with_difference(*size, reference, measurement.kind)
        });
        let tools = measurement.build_tool.map_or_else(
            || "cargo".to_owned(),
            |tool| format!("cargo, {tool}, git, network"),
        );
        let cells = sizes.chain([measurement.dependency_count.to_string(), tools]);
        print_row(&measurement.name, cells);
    }
}

fn print_conformance(measurements: &[Measurement]) {
    let terminals: Vec<&Measurement> = measurements
        .iter()
        .filter(|m| m.kind == Kind::Terminal)
        .collect();
    let Some(first) = terminals.first() else {
        return;
    };
    println!("\n## Conformance\n");
    let names: Vec<&str> = terminals.iter().map(|m| m.name.as_str()).collect();
    print_header("Case", &names);
    for (index, case) in first.conformance.iter().enumerate() {
        let cells = terminals.iter().map(|m| {
            let verdict = if m.conformance[index].passed {
                "pass"
            } else {
                "**fail**"
            };
            verdict.to_owned()
        });
        print_row(&case.name, cells);
    }
    let totals = terminals.iter().map(|m| {
        let passed = m.conformance.iter().filter(|case| case.passed).count();
        format!("{passed} of {}", m.conformance.len())
    });
    print_row("Passed", totals);
}

/// A value with its difference from the baseline in brackets.
fn with_difference(value: u64, reference: u64, kind: Kind) -> String {
    match kind {
        Kind::Baseline => value.to_string(),
        _ => format!("{value} (+{})", value.saturating_sub(reference)),
    }
}

fn print_header(first: &str, columns: &[impl AsRef<str>]) {
    print_row(
        first,
        columns.iter().map(|column| column.as_ref().to_owned()),
    );
    println!("|---|{}", "---|".repeat(columns.len()));
}

fn print_row(first: &str, cells: impl Iterator<Item = String>) {
    let cells: Vec<String> = cells.collect();
    println!("| {first} | {} |", cells.join(" | "));
}
