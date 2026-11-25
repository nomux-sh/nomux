//! All measurements of one candidate.

use std::error::Error;
use std::process::Command;

use vt_probe::Corpus;

use crate::candidates::{Candidate, Kind};
use crate::probe::{CaseResult, HeldTerminal, Probe};
use crate::workspace::{PROFILES, Workspace};

/// The scrollback sizes of the memory measurement. 0 is the screen only.
pub(crate) const SCROLLBACK_LINES: [usize; 3] = [0, 1000, 10_000];

#[derive(Debug)]
pub(crate) struct Measurement {
    pub(crate) name: String,
    pub(crate) kind: Kind,
    pub(crate) build_tool: Option<&'static str>,
    pub(crate) dependency_count: usize,
    /// The size of the probe binary in bytes for each profile in [`PROFILES`].
    pub(crate) sizes: Vec<u64>,
    /// MiB/s for each corpus in [`Corpus::ALL`]. Empty for the baseline.
    pub(crate) throughput: Vec<f64>,
    /// One terminal for each entry in [`SCROLLBACK_LINES`].
    pub(crate) terminals: Vec<HeldTerminal>,
    /// Empty for a candidate without screen state.
    pub(crate) conformance: Vec<CaseResult>,
}

#[derive(Debug)]
pub(crate) enum Outcome {
    Measured(Measurement),
    NotBuilt {
        package: &'static str,
        reason: String,
    },
}

pub(crate) fn measure(
    workspace: &Workspace,
    candidate: &Candidate,
) -> Result<Outcome, Box<dyn Error>> {
    if let Some(tool) = candidate.build_tool.filter(|tool| !is_installed(tool)) {
        return Ok(Outcome::NotBuilt {
            package: candidate.package,
            reason: format!("`{tool}` is not installed"),
        });
    }
    let mut probes = Vec::new();
    for profile in PROFILES {
        probes.push(Probe::new(workspace.build(candidate.package, profile)?));
    }
    let sizes = probes
        .iter()
        .map(Probe::size_bytes)
        .collect::<Result<_, _>>()?;
    // The speed and the memory are those of the profile of the core workspace.
    let probe = &probes[0];
    Ok(Outcome::Measured(Measurement {
        name: probe.name()?,
        kind: candidate.kind,
        build_tool: candidate.build_tool,
        dependency_count: workspace.dependency_count(candidate.package)?,
        sizes,
        throughput: throughput(probe, candidate.kind)?,
        terminals: terminals(probe)?,
        conformance: conformance(probe, candidate.kind)?,
    }))
}

fn is_installed(tool: &str) -> bool {
    Command::new(tool)
        .arg("version")
        .output()
        .is_ok_and(|output| output.status.success())
}

fn throughput(probe: &Probe, kind: Kind) -> Result<Vec<f64>, Box<dyn Error>> {
    if kind == Kind::Baseline {
        return Ok(Vec::new());
    }
    Corpus::ALL
        .into_iter()
        .map(|corpus| probe.throughput(corpus))
        .collect()
}

fn terminals(probe: &Probe) -> Result<Vec<HeldTerminal>, Box<dyn Error>> {
    SCROLLBACK_LINES
        .into_iter()
        .map(|lines| probe.hold_terminal(lines))
        .collect()
}

fn conformance(probe: &Probe, kind: Kind) -> Result<Vec<CaseResult>, Box<dyn Error>> {
    if kind != Kind::Terminal {
        return Ok(Vec::new());
    }
    probe.conformance()
}
