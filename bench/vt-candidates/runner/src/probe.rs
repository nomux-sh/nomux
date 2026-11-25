//! The commands of one probe binary.

use std::error::Error;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};

use vt_probe::Corpus;

const THROUGHPUT_BYTES: u32 = 16 * 1024 * 1024;
const THROUGHPUT_RUNS: u32 = 5;
const BYTES_IN_MEBIBYTE: f64 = 1_048_576.0;
const NANOSECONDS_IN_SECOND: f64 = 1e9;

/// The result of one conformance case.
#[derive(Debug)]
pub(crate) struct CaseResult {
    pub(crate) name: String,
    pub(crate) passed: bool,
}

/// One terminal that a probe filled and kept alive for the measurement.
#[derive(Debug)]
pub(crate) struct HeldTerminal {
    pub(crate) scrollback_lines: usize,
    /// The scrollback lines that the candidate kept.
    pub(crate) retained_lines: usize,
    /// The resident set size of the probe process in KiB.
    pub(crate) resident_kib: u64,
}

pub(crate) struct Probe {
    binary: PathBuf,
}

impl Probe {
    pub(crate) fn new(binary: PathBuf) -> Self {
        Self { binary }
    }

    pub(crate) fn name(&self) -> Result<String, Box<dyn Error>> {
        Ok(self.output(&["name"])?.trim().to_owned())
    }

    /// The parse throughput on one corpus in MiB/s.
    pub(crate) fn throughput(&self, corpus: Corpus) -> Result<f64, Box<dyn Error>> {
        let bytes = THROUGHPUT_BYTES.to_string();
        let runs = THROUGHPUT_RUNS.to_string();
        let output = self.output(&["throughput", corpus.name(), &bytes, &runs])?;
        let mut fields = output.split_whitespace();
        let bytes: f64 = fields.next().ok_or("no corpus size")?.parse()?;
        let nanoseconds: f64 = fields.next().ok_or("no parse time")?.parse()?;
        Ok(bytes / BYTES_IN_MEBIBYTE / (nanoseconds / NANOSECONDS_IN_SECOND))
    }

    pub(crate) fn conformance(&self) -> Result<Vec<CaseResult>, Box<dyn Error>> {
        let output = self.output(&["conformance"])?;
        let results = output.lines().filter_map(|line| {
            let mut fields = line.split('\t');
            let passed = fields.next()? == "pass";
            let name = fields.next()?.to_owned();
            Some(CaseResult { name, passed })
        });
        Ok(results.collect())
    }

    /// Starts the probe with a full screen and `scrollback_lines` lines of
    /// scrollback, and measures the process while it waits.
    pub(crate) fn hold_terminal(
        &self,
        scrollback_lines: usize,
    ) -> Result<HeldTerminal, Box<dyn Error>> {
        let mut child = Command::new(&self.binary)
            .args(["memory", &scrollback_lines.to_string()])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()?;
        let stdout = child
            .stdout
            .take()
            .ok_or("the probe has no standard output")?;
        let mut ready = String::new();
        BufReader::new(stdout).read_line(&mut ready)?;
        let retained_lines = ready
            .strip_prefix("ready ")
            .ok_or("the probe did not print `ready`")?
            .trim()
            .parse()?;
        let resident_kib = resident_kib(child.id());
        // The probe exits when its standard input closes.
        drop(child.stdin.take());
        child.wait()?;
        Ok(HeldTerminal {
            scrollback_lines,
            retained_lines,
            resident_kib: resident_kib?,
        })
    }

    pub(crate) fn size_bytes(&self) -> Result<u64, Box<dyn Error>> {
        Ok(self.binary.metadata()?.len())
    }

    fn output(&self, arguments: &[&str]) -> Result<String, Box<dyn Error>> {
        let output = Command::new(&self.binary)
            .args(arguments)
            .stderr(Stdio::inherit())
            .output()?;
        if !output.status.success() {
            return Err(format!(
                "{} {arguments:?} failed: {}",
                self.binary.display(),
                output.status
            )
            .into());
        }
        Ok(String::from_utf8(output.stdout)?)
    }
}

/// The resident set size of a process in KiB. `ps` has this output format on
/// Linux and on macOS.
fn resident_kib(process_id: u32) -> Result<u64, Box<dyn Error>> {
    let output = Command::new("ps")
        .args(["-o", "rss=", "-p", &process_id.to_string()])
        .output()?;
    Ok(String::from_utf8(output.stdout)?.trim().parse()?)
}
